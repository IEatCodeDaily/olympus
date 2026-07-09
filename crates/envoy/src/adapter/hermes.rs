//! Hermes setup adapter — the first concrete impl (ADR 0006 §9.2).
//!
//! Hermes materialization:
//! - MCP servers → returned as ACP `session/new` `mcpServers` JSON (the bridge
//!   currently sends `[]`; we populate it from the registry definitions).
//! - Skills → the definition holds a directory path; Hermes's `~/.hermes/skills/`
//!   is the baseline (union), so we symlink/copy session-scoped skill dirs into
//!   the session space and set HERMES_SKILLS_PATH to point at them.
//! - Hooks/plugins → Hermes doesn't have native session-scoped hooks/plugins via
//!   ACP; these are FallbackToContext (appended to a context note) or
//!   Unsupported.

use std::path::Path;

use anyhow::{Context, Result};

use super::{AgentKind, Capabilities, MergeMode, SetupAdapter, SpawnOverlay, Support};

pub struct HermesAdapter;

impl SetupAdapter for HermesAdapter {
    fn agent_kind(&self) -> AgentKind {
        AgentKind::Hermes
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            skills: Support::Native,
            mcp: Support::Native,
            hooks: Support::Native,
            plugins: Support::FallbackToContext,
        }
    }

    fn materialize(
        &self,
        resolved: &super::ResolvedSetup,
        space: &Path,
        mode: MergeMode,
    ) -> Result<SpawnOverlay> {
        let mut overlay = SpawnOverlay::default();

        // 1. MCP servers → parse each definition JSON and collect into the
        //    mcpServers array the ACP session/new will carry. The bridge's
        //    build_session_new_request currently sends "mcpServers": []; the
        //    factory will read overlay.mcp_servers and populate it.
        for entry in &resolved.mcp.resolved {
            match serde_json::from_str::<serde_json::Value>(&entry.definition) {
                Ok(def) => overlay.mcp_servers.push(def),
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

        // 2. Skills → the definition is a JSON object with a "dir" field
        //    pointing at the skill directory. Hermes's baseline is
        //    ~/.hermes/skills/ (union mode). For session-scoped activation,
        //    symlink each resolved skill dir into the session space's .skills/
        //    and set HERMES_SKILLS_PATH so the agent discovers them.
        let skills_dir = space.join(".skills");
        if !resolved.skills.resolved.is_empty() {
            std::fs::create_dir_all(&skills_dir)
                .with_context(|| format!("creating skills dir {}", skills_dir.display()))?;
        }
        let mut skill_paths: Vec<String> = Vec::new();
        for entry in &resolved.skills.resolved {
            match parse_skill_dir(&entry.definition) {
                Ok(dir) => {
                    let link = skills_dir.join(&entry.slug);
                    // Symlink (best effort — if it exists, skip).
                    let _ = std::os::unix::fs::symlink(&dir, &link);
                    skill_paths.push(link.to_string_lossy().into_owned());
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
        // In union mode, the agent keeps its baseline skills; in override mode,
        // HERMES_SKILLS_PATH replaces them entirely. We always set the env var
        // when skills are materialized.
        if !skill_paths.is_empty() {
            let joined = skill_paths.join(":");
            overlay.env.push(("HERMES_SKILLS_PATH".into(), joined));
        }

        // 3. Hooks → Hermes supports hooks via profile config, but not
        //    session-scoped via ACP. For now, materialize as context
        //    (FallbackToContext isn't applicable to hooks directly, but we
        //    surface the declared hooks as a note the agent sees).
        //    TODO: when Hermes ACP gains session-scoped hooks, materialize
        //    natively here.
        if !resolved.hooks.resolved.is_empty() {
            let hook_names: Vec<&str> = resolved
                .hooks
                .resolved
                .iter()
                .map(|e| e.slug.as_str())
                .collect();
            overlay.warnings.push(format!(
                "hooks declared ({}) but Hermes ACP has no session-scoped hooks yet; \
                 these are inactive for this session",
                hook_names.join(", ")
            ));
        }
        for slug in &resolved.hooks.unresolved {
            overlay
                .warnings
                .push(format!("hook/{slug}: not in registry, skipping"));
        }

        // 4. Plugins (LSP/codegraph/install) → Hermes doesn't have native
        //    session-scoped plugin activation via ACP. FallbackToContext: note
        //    them as context so the agent is aware.
        if !resolved.plugins.resolved.is_empty() {
            let plugin_names: Vec<&str> = resolved
                .plugins
                .resolved
                .iter()
                .map(|e| e.slug.as_str())
                .collect();
            overlay.warnings.push(format!(
                "plugins declared ({}) — Hermes has no session-scoped plugin activation; \
                 ensure these are installed at the node level",
                plugin_names.join(", ")
            ));
        }
        for slug in &resolved.plugins.unresolved {
            overlay
                .warnings
                .push(format!("plugin/{slug}: not in registry, skipping"));
        }

        // MergeMode is advisory for Hermes — it's always union (Hermes ACP
        // can't suppress profile skills). We record it for future use.
        let _ = mode;

        Ok(overlay)
    }
}

/// Parse a skill definition JSON to extract the skill directory path.
/// Expected shape: `{"dir": "/path/to/skill"}` or
/// `{"dir": "/path", "name": "...", "description": "..."}`.
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
    fn hermes_materializes_mcp_servers() {
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
        let adapter = HermesAdapter;
        let overlay = adapter
            .materialize(&resolved, tmp.path(), MergeMode::Union)
            .unwrap();
        assert_eq!(overlay.mcp_servers.len(), 2);
        assert!(overlay.warnings.iter().any(|w| w.contains("mcp/unknown")));
    }

    #[test]
    fn hermes_materializes_skills_with_env() {
        let space = tempfile::tempdir().unwrap();
        // Create a fake skill dir.
        let skill_dir = space.path().join("fake-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let resolved = ResolvedSetup {
            skills: ResolvedCategory {
                resolved: vec![skill_entry("code-review", skill_dir.to_str().unwrap())],
                unresolved: vec![],
            },
            ..Default::default()
        };
        let adapter = HermesAdapter;
        let overlay = adapter
            .materialize(&resolved, space.path(), MergeMode::Union)
            .unwrap();
        assert!(overlay.env.iter().any(|(k, _)| k == "HERMES_SKILLS_PATH"));
        // The symlink was created.
        assert!(space.path().join(".skills/code-review").exists());
    }

    #[test]
    fn hermes_warns_on_missing_skill_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = ResolvedSetup {
            skills: ResolvedCategory {
                resolved: vec![skill_entry("ghost", "/nonexistent/path")],
                unresolved: vec![],
            },
            ..Default::default()
        };
        let adapter = HermesAdapter;
        let overlay = adapter
            .materialize(&resolved, tmp.path(), MergeMode::Union)
            .unwrap();
        assert!(overlay
            .warnings
            .iter()
            .any(|w| w.contains("does not exist")));
    }

    #[test]
    fn hermes_capabilities() {
        let cap = HermesAdapter.capabilities();
        assert_eq!(cap.skills, Support::Native);
        assert_eq!(cap.mcp, Support::Native);
        assert_eq!(cap.plugins, Support::FallbackToContext);
    }

    #[test]
    fn hermes_hooks_surface_as_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = ResolvedSetup {
            hooks: ResolvedCategory {
                resolved: vec![RegistryEntry {
                    kind: "hook".into(),
                    slug: "pre-commit".into(),
                    definition: "{}".into(),
                    registered_at: 1.0,
                }],
                unresolved: vec![],
            },
            ..Default::default()
        };
        let adapter = HermesAdapter;
        let overlay = adapter
            .materialize(&resolved, tmp.path(), MergeMode::Union)
            .unwrap();
        assert!(overlay
            .warnings
            .iter()
            .any(|w| w.contains("session-scoped hooks")));
    }

    #[test]
    fn hermes_bad_mcp_json_warns() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = ResolvedSetup {
            mcp: ResolvedCategory {
                resolved: vec![mcp_entry("broken", "not json")],
                unresolved: vec![],
            },
            ..Default::default()
        };
        let adapter = HermesAdapter;
        let overlay = adapter
            .materialize(&resolved, tmp.path(), MergeMode::Union)
            .unwrap();
        assert!(overlay.mcp_servers.is_empty());
        assert!(overlay
            .warnings
            .iter()
            .any(|w| w.contains("not valid JSON")));
    }
}
