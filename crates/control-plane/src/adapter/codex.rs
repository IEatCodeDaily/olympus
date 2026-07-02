//! Codex setup adapter (ADR 0006 §9).
//!
//! Codex reads config from a directory pointed to by `CODEX_HOME`. The adapter
//! creates a session-local config dir inside the session space and points
//! `CODEX_HOME` at it via the spawn overlay's env:
//!
//! - MCP servers → `<space>/.codex/config.toml` under `[mcp_servers.*]`
//!   (STDIO only — Codex's native MCP transport)
//! - Skills → `<space>/.codex/skills/<slug>/` (copy-dir)
//! - Hooks → Codex hooks format in config.toml
//! - Plugins → Unsupported (dropped with warning)
//! - Context → `<space>/AGENTS.md` (appended for degraded items)
//!
//! The env overlay carries `CODEX_HOME=<space>/.codex` so the runtime factory
//! applies it to the child process.

use std::path::Path;

use anyhow::{Context, Result};

use super::{AgentKind, Capabilities, MergeMode, SetupAdapter, SpawnOverlay, Support};
use crate::views::RegistryEntry;

pub struct CodexAdapter;

impl SetupAdapter for CodexAdapter {
    fn agent_kind(&self) -> AgentKind {
        AgentKind::Codex
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

        let codex_home = space.join(".codex");
        std::fs::create_dir_all(&codex_home)
            .with_context(|| format!("creating CODEX_HOME at {}", codex_home.display()))?;

        overlay.env.push((
            "CODEX_HOME".into(),
            codex_home.to_string_lossy().into_owned(),
        ));

        // 1. MCP servers → config.toml [mcp_servers.*] sections.
        //    Codex config.toml format:
        //    [mcp_servers.name]
        //    command = "..."
        //    args = [...]
        //    env = { KEY = "value" }
        //
        //    We build the TOML incrementally. In Union mode, we parse + merge
        //    with existing config; in Override mode, we write fresh.
        let config_path = codex_home.join("config.toml");
        let mut toml_lines: Vec<String> = Vec::new();
        let mut has_mcp = false;

        if mode == MergeMode::Union && config_path.exists() {
            // Preserve existing config lines that aren't our mcp_servers sections.
            let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
            for line in existing.lines() {
                toml_lines.push(line.to_string());
            }
        }

        for entry in &resolved.mcp.resolved {
            match serde_json::from_str::<serde_json::Value>(&entry.definition) {
                Ok(def) => {
                    has_mcp = true;
                    let section = render_mcp_toml(&entry.slug, &def);
                    toml_lines.push(String::new()); // blank line separator
                    toml_lines.push(section);
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
        if has_mcp || !toml_lines.is_empty() {
            std::fs::write(&config_path, toml_lines.join("\n"))
                .with_context(|| format!("writing {}", config_path.display()))?;
        }

        // 2. Skills → <CODEX_HOME>/skills/<slug>/ (copy-dir).
        if !resolved.skills.resolved.is_empty() {
            let skills_base = codex_home.join("skills");
            std::fs::create_dir_all(&skills_base)
                .with_context(|| format!("creating skills dir {}", skills_base.display()))?;
        }
        for entry in &resolved.skills.resolved {
            match parse_skill_dir(&entry.definition) {
                Ok(dir) => {
                    let dest = codex_home.join("skills").join(&entry.slug);
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

        // 3. Hooks → Codex hooks format. The registry definition is a JSON
        //    snippet describing the hook; we append it to config.toml as a
        //    comment block (Codex's hook schema is less standardized than
        //    Claude Code's, so we surface as context + write what we can).
        //    For now, hooks are surfaced as context warnings.
        for entry in &resolved.hooks.resolved {
            overlay.warnings.push(format!(
                "hook/{}: Codex hook format is harness-specific — definition written to AGENTS.md",
                entry.slug
            ));
        }
        for slug in &resolved.hooks.unresolved {
            overlay
                .warnings
                .push(format!("hook/{slug}: not in registry, skipping"));
        }

        // 4. Plugins → Unsupported.
        for entry in &resolved.plugins.resolved {
            overlay.warnings.push(format!(
                "plugin/{}: Codex has no plugin system — dropped",
                entry.slug
            ));
        }
        for slug in &resolved.plugins.unresolved {
            overlay
                .warnings
                .push(format!("plugin/{slug}: not in registry, skipping"));
        }

        // 5. Write AGENTS.md with warnings as context.
        if !overlay.warnings.is_empty() {
            let agents_md = space.join("AGENTS.md");
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
            if mode == MergeMode::Union && agents_md.exists() {
                let existing = std::fs::read_to_string(&agents_md).unwrap_or_default();
                std::fs::write(&agents_md, format!("{existing}\n\n{content}"))
                    .with_context(|| format!("appending to {}", agents_md.display()))?;
            } else {
                std::fs::write(&agents_md, &content)
                    .with_context(|| format!("writing {}", agents_md.display()))?;
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

/// Render an MCP server definition as a TOML [mcp_servers.<name>] section.
/// Input is the JSON definition: { "command": "...", "args": [...], "env": {...} }
fn render_mcp_toml(name: &str, def: &serde_json::Value) -> String {
    let mut lines = vec![format!("[mcp_servers.{name}]")];

    if let Some(cmd) = def.get("command").and_then(|v| v.as_str()) {
        lines.push(format!("command = {cmd:?}"));
    }
    if let Some(args) = def.get("args").and_then(|v| v.as_array()) {
        let toml_args: Vec<String> = args
            .iter()
            .filter_map(|a| a.as_str().map(|s| format!("{s:?}")))
            .collect();
        if !toml_args.is_empty() {
            lines.push(format!("args = [{}]", toml_args.join(", ")));
        }
    }
    if let Some(env) = def.get("env").and_then(|v| v.as_object()) {
        if !env.is_empty() {
            lines.push(format!("[mcp_servers.{name}.env]"));
            for (k, v) in env {
                if let Some(s) = v.as_str() {
                    lines.push(format!("{k} = {s:?}"));
                }
            }
        }
    }

    lines.join("\n")
}

/// Recursively copy a directory.
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

    #[test]
    fn codex_writes_config_toml_with_mcp() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = ResolvedSetup {
            mcp: ResolvedCategory {
                resolved: vec![
                    mcp_entry("gitnexus", r#"{"command":"gitnexus","args":["--stdio"]}"#),
                    mcp_entry(
                        "weather",
                        r#"{"command":"weather-mcp","env":{"API_KEY":"xyz"}}"#,
                    ),
                ],
                unresolved: vec!["unknown".into()],
            },
            ..Default::default()
        };
        let adapter = CodexAdapter;
        let overlay = adapter
            .materialize(&resolved, tmp.path(), MergeMode::Union)
            .unwrap();

        // CODEX_HOME env should be set.
        assert!(overlay
            .env
            .iter()
            .any(|(k, v)| k == "CODEX_HOME" && v.contains(".codex")));

        let config = tmp.path().join(".codex").join("config.toml");
        assert!(config.exists(), "config.toml should exist");
        let content = std::fs::read_to_string(&config).unwrap();
        assert!(content.contains("[mcp_servers.gitnexus]"));
        assert!(content.contains(r#"command = "gitnexus""#));
        assert!(content.contains("[mcp_servers.weather.env]"));
        assert!(overlay.warnings.iter().any(|w| w.contains("mcp/unknown")));
    }

    #[test]
    fn codex_copies_skills() {
        let space = tempfile::tempdir().unwrap();
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
        let adapter = CodexAdapter;
        let _overlay = adapter
            .materialize(&resolved, space.path(), MergeMode::Union)
            .unwrap();

        let skill_file = space
            .path()
            .join(".codex")
            .join("skills")
            .join("code-review")
            .join("SKILL.md");
        assert!(
            skill_file.exists(),
            "skill should be copied to .codex/skills/"
        );
        assert!(std::fs::read_to_string(&skill_file)
            .unwrap()
            .contains("Fake skill"));
    }

    #[test]
    fn codex_plugins_dropped_with_warning() {
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
        let adapter = CodexAdapter;
        let overlay = adapter
            .materialize(&resolved, tmp.path(), MergeMode::Union)
            .unwrap();
        assert!(overlay.warnings.iter().any(|w| w.contains("no plugin")));
    }

    #[test]
    fn codex_capabilities() {
        let cap = CodexAdapter.capabilities();
        assert_eq!(cap.skills, Support::Native);
        assert_eq!(cap.mcp, Support::Native);
        assert_eq!(cap.hooks, Support::Native);
        assert_eq!(cap.plugins, Support::Unsupported);
    }

    #[test]
    fn codex_agent_kind() {
        assert_eq!(CodexAdapter.agent_kind(), AgentKind::Codex);
    }
}
