//! Claude Code setup adapter (ADR 0006 §9).
//!
//! Claude Code reads all config from the **cwd** — so the session space IS
//! the session-scope lever, natively. The adapter writes:
//!
//! - MCP servers → `<space>/.mcp.json` (Claude Code's project-level MCP config)
//! - Skills → `<space>/.claude/skills/<slug>/` (copy-dir from registry `dir`)
//! - Hooks → `<space>/.claude/settings.json` (hooks block)
//! - Plugins → Unsupported (dropped with warning)
//! - Fallback context → `<space>/CLAUDE.md` (appended for items that degrade)
//!
//! MergeMode: Override rewrites the files from scratch; Union merges with any
//! existing files in the session space. In practice, the session space is
//! fresh at materialization, so Union and Override produce the same result
//! unless the operator pre-seeded config.

use std::path::Path;

use anyhow::{Context, Result};

use super::{AgentKind, Capabilities, MergeMode, SetupAdapter, SpawnOverlay, Support};
use crate::views::RegistryEntry;

pub struct ClaudeCodeAdapter;

impl SetupAdapter for ClaudeCodeAdapter {
    fn agent_kind(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            skills: Support::Native,
            mcp: Support::Native,
            hooks: Support::Native,
            plugins: Support::Unsupported,
        }
    }

    fn materialize(
        &self,
        resolved: &super::ResolvedSetup,
        space: &Path,
        mode: MergeMode,
    ) -> Result<SpawnOverlay> {
        let mut overlay = SpawnOverlay::default();

        // 1. MCP servers → .mcp.json in the session-space root.
        //    Claude Code's .mcp.json format:
        //    { "mcpServers": { "name": { "command": "...", "args": [...], "env": {...} } } }
        let mut mcp_map = serde_json::Map::new();
        for entry in &resolved.mcp.resolved {
            match serde_json::from_str::<serde_json::Value>(&entry.definition) {
                Ok(def) => {
                    mcp_map.insert(entry.slug.clone(), def);
                }
                Err(e) => {
                    overlay.warnings.push(format!(
                        "mcp/{}: definition is not valid JSON ({e})",
                        entry.slug
                    ));
                }
            }
        }
        for slug in &resolved.mcp.unresolved {
            overlay
                .warnings
                .push(format!("mcp/{slug}: not in registry, skipping"));
        }
        if !mcp_map.is_empty() {
            let mcp_file = serde_json::json!({ "mcpServers": mcp_map });
            let mcp_path = space.join(".mcp.json");
            std::fs::write(&mcp_path, serde_json::to_string_pretty(&mcp_file)?)
                .with_context(|| format!("writing {}", mcp_path.display()))?;
        }

        // 2. Skills → .claude/skills/<slug>/ (copy the skill dir).
        //    Skills are portable: same SKILL.md shape. Claude Code reads them
        //    from .claude/skills/ in the cwd. We copy (not symlink) because
        //    Claude Code may resolve symlinks oddly and the registry dir is
        //    the authoritative source.
        if !resolved.skills.resolved.is_empty() {
            let skills_base = space.join(".claude").join("skills");
            std::fs::create_dir_all(&skills_base)
                .with_context(|| format!("creating skills dir {}", skills_base.display()))?;
        }
        for entry in &resolved.skills.resolved {
            match parse_skill_dir(&entry.definition) {
                Ok(dir) => {
                    let dest = space.join(".claude").join("skills").join(&entry.slug);
                    copy_dir_recursive(Path::new(&dir), &dest)
                        .with_context(|| format!("copying skill {} to {}", dir, dest.display()))?;
                }
                Err(e) => {
                    overlay
                        .warnings
                        .push(format!("skill/{}: {}", entry.slug, e));
                }
            }
        }
        for slug in &resolved.skills.unresolved {
            overlay
                .warnings
                .push(format!("skill/{slug}: not in registry, skipping"));
        }

        // 3. Hooks → .claude/settings.json (hooks block).
        //    Claude Code's settings.json supports hooks natively. The registry
        //    definition for a hook is a JSON snippet matching the hooks schema
        //    fragment. We merge all declared hooks into a single settings.json.
        if !resolved.hooks.resolved.is_empty() {
            let settings_path = space.join(".claude").join("settings.json");
            std::fs::create_dir_all(space.join(".claude"))
                .with_context(|| format!("creating .claude dir"))?;

            let mut settings: serde_json::Value = if mode == MergeMode::Union {
                std::fs::read(&settings_path)
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_else(|| serde_json::json!({}))
            } else {
                serde_json::json!({})
            };

            if !settings.is_object() {
                settings = serde_json::json!({});
            }
            let settings_obj = settings.as_object_mut().unwrap();
            let hooks = settings_obj
                .entry("hooks")
                .or_insert_with(|| serde_json::json!({}));
            if !hooks.is_object() {
                *hooks = serde_json::json!({});
            }
            let hooks_obj = hooks.as_object_mut().unwrap();

            for entry in &resolved.hooks.resolved {
                match serde_json::from_str::<serde_json::Value>(&entry.definition) {
                    Ok(def) => {
                        // The hook definition is expected to be a sub-object
                        // (e.g. {"PreToolUse": [...]}). Merge top-level keys.
                        if let Some(obj) = def.as_object() {
                            for (k, v) in obj {
                                hooks_obj.insert(k.clone(), v.clone());
                            }
                        } else {
                            // If it's not an object, store it under the slug.
                            hooks_obj.insert(entry.slug.clone(), def);
                        }
                    }
                    Err(e) => {
                        overlay
                            .warnings
                            .push(format!("hook/{}: invalid JSON ({e})", entry.slug));
                    }
                }
            }
            for slug in &resolved.hooks.unresolved {
                overlay
                    .warnings
                    .push(format!("hook/{slug}: not in registry, skipping"));
            }

            std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)
                .with_context(|| format!("writing {}", settings_path.display()))?;
        }

        // 4. Plugins → Unsupported. Surface as warning.
        for entry in &resolved.plugins.resolved {
            overlay.warnings.push(format!(
                "plugin/{}: Claude Code has no plugin system — dropped",
                entry.slug
            ));
        }
        for slug in &resolved.plugins.unresolved {
            overlay
                .warnings
                .push(format!("plugin/{slug}: not in registry, skipping"));
        }

        // 5. Write CLAUDE.md if we have warnings to surface as context.
        //    Operators can read this to understand what was declared but
        //    couldn't be materialized.
        if !overlay.warnings.is_empty() {
            let claude_md = space.join("CLAUDE.md");
            let content = format!(
                "# Session setup notes\n\n\
                 This session was materialized by Olympus. The following items were \
                 declared but could not be fully activated:\n\n{}\n",
                overlay
                    .warnings
                    .iter()
                    .map(|w| format!("- {w}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            // Append to existing CLAUDE.md if present (Union), else create.
            if mode == MergeMode::Union && claude_md.exists() {
                let existing = std::fs::read_to_string(&claude_md).unwrap_or_default();
                std::fs::write(&claude_md, format!("{existing}\n\n{content}"))
                    .with_context(|| format!("appending to {}", claude_md.display()))?;
            } else {
                std::fs::write(&claude_md, &content)
                    .with_context(|| format!("writing {}", claude_md.display()))?;
            }
        }

        Ok(overlay)
    }
}

/// Parse a skill definition JSON to extract the skill directory path.
fn parse_skill_dir(definition: &str) -> Result<String> {
    let v: serde_json::Value =
        serde_json::from_str(definition).with_context(|| "skill definition is not valid JSON")?;
    let dir = v
        .get("dir")
        .and_then(|d| d.as_str())
        .ok_or_else(|| anyhow::anyhow!("skill definition missing 'dir' field"))?;
    if !Path::new(dir).exists() {
        return Err(anyhow::anyhow!("skill dir does not exist: {dir}"));
    }
    Ok(dir.to_string())
}

/// Recursively copy a directory. Used for skill dirs (portable content).
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ResolvedCategory, ResolvedSetup};
    use crate::views::RegistryEntry;

    fn mcp_entry(slug: &str, def: &str) -> RegistryEntry {
        RegistryEntry {
            kind: "mcp".into(),
            slug: slug.into(),
            definition: def.into(),
            registered_at: 1.0,
        }
    }

    fn skill_entry(slug: &str, dir: &str) -> RegistryEntry {
        RegistryEntry {
            kind: "skill".into(),
            slug: slug.into(),
            definition: format!(r#"{{"dir":"{dir}"}}"#),
            registered_at: 1.0,
        }
    }

    fn hook_entry(slug: &str, def: &str) -> RegistryEntry {
        RegistryEntry {
            kind: "hook".into(),
            slug: slug.into(),
            definition: def.into(),
            registered_at: 1.0,
        }
    }

    #[test]
    fn claude_code_writes_mcp_json() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = ResolvedSetup {
            mcp: ResolvedCategory {
                resolved: vec![
                    mcp_entry("gitnexus", r#"{"command":"gitnexus","args":["--stdio"]}"#),
                    mcp_entry("grafana", r#"{"command":"grafana-mcp"}"#),
                ],
                unresolved: vec!["unknown".into()],
            },
            ..Default::default()
        };
        let adapter = ClaudeCodeAdapter;
        let overlay = adapter
            .materialize(&resolved, tmp.path(), MergeMode::Union)
            .unwrap();
        let mcp_file = tmp.path().join(".mcp.json");
        assert!(mcp_file.exists(), ".mcp.json should exist");
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&mcp_file).unwrap()).unwrap();
        assert!(content["mcpServers"]["gitnexus"]["command"].as_str() == Some("gitnexus"));
        assert!(overlay.warnings.iter().any(|w| w.contains("mcp/unknown")));
    }

    #[test]
    fn claude_code_copies_skills() {
        let space = tempfile::tempdir().unwrap();
        // Create a fake skill dir with a SKILL.md.
        let skill_src = space.path().join("fake-skill-src");
        std::fs::create_dir_all(&skill_src).unwrap();
        std::fs::write(
            skill_src.join("SKILL.md"),
            "---\nname: fake\ndescription: test\n---\n# Fake skill\n",
        )
        .unwrap();

        let resolved = ResolvedSetup {
            skills: ResolvedCategory {
                resolved: vec![skill_entry("code-review", skill_src.to_str().unwrap())],
                unresolved: vec![],
            },
            ..Default::default()
        };
        let adapter = ClaudeCodeAdapter;
        let _overlay = adapter
            .materialize(&resolved, space.path(), MergeMode::Union)
            .unwrap();

        let skill_file = space
            .path()
            .join(".claude")
            .join("skills")
            .join("code-review")
            .join("SKILL.md");
        assert!(
            skill_file.exists(),
            "skill should be copied to .claude/skills/"
        );
        assert!(std::fs::read_to_string(&skill_file)
            .unwrap()
            .contains("Fake skill"));
    }

    #[test]
    fn claude_code_writes_hooks_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = ResolvedSetup {
            hooks: ResolvedCategory {
                resolved: vec![hook_entry(
                    "pre-commit",
                    r#"{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"echo hi"}]}]}"#,
                )],
                unresolved: vec![],
            },
            ..Default::default()
        };
        let adapter = ClaudeCodeAdapter;
        let _overlay = adapter
            .materialize(&resolved, tmp.path(), MergeMode::Union)
            .unwrap();
        let settings = tmp.path().join(".claude").join("settings.json");
        assert!(settings.exists(), "settings.json should exist");
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert!(content["hooks"]["PreToolUse"].is_array());
    }

    #[test]
    fn claude_code_plugins_dropped_with_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = ResolvedSetup {
            plugins: ResolvedCategory {
                resolved: vec![RegistryEntry {
                    kind: "plugin".into(),
                    slug: "lsp".into(),
                    definition: "{}".into(),
                    registered_at: 1.0,
                }],
                unresolved: vec![],
            },
            ..Default::default()
        };
        let adapter = ClaudeCodeAdapter;
        let overlay = adapter
            .materialize(&resolved, tmp.path(), MergeMode::Union)
            .unwrap();
        assert!(overlay.warnings.iter().any(|w| w.contains("no plugin")));
    }

    #[test]
    fn claude_code_capabilities() {
        let cap = ClaudeCodeAdapter.capabilities();
        assert_eq!(cap.skills, Support::Native);
        assert_eq!(cap.mcp, Support::Native);
        assert_eq!(cap.hooks, Support::Native);
        assert_eq!(cap.plugins, Support::Unsupported);
    }

    #[test]
    fn claude_code_agent_kind() {
        assert_eq!(ClaudeCodeAdapter.agent_kind(), AgentKind::ClaudeCode);
    }
}
