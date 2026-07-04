//! Agent discovery — lists the Hermes "agents" (profiles) Olympus can drive,
//! with their configured provider + model, so the UI can offer a real
//! provider/model picker instead of a hardcoded list.
//!
//! An "agent" in Hermes is a profile: `~/.hermes/profiles/<name>/config.yaml`
//! plus the implicit root profile (`~/.hermes/config.yaml`, exposed as the
//! `default` agent). We parse the small `model:` block (default/provider/
//! base_url) with a line scanner rather than pulling in a YAML dependency —
//! the block shape is stable and this avoids the deprecated serde_yaml.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use serde::Serialize;

const CLAUDE_CODE_AGENT_ID: &str = "claude-code";
const CODEX_AGENT_ID: &str = "codex";

/// One drivable agent (Hermes profile or local CLI harness) as the UI consumes it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// Agent id passed back in `POST /api/sessions { agent }`.
    pub id: String,
    /// Configured provider (e.g. "anthropic", "openai-codex", "zai").
    pub provider: Option<String>,
    /// Configured default model, or the discovered CLI version for CLI harnesses.
    pub model: Option<String>,
    /// Agent harness kind: "hermes", "claude-code", or "codex".
    pub kind: String,
    /// Whether this is the implicit root profile the server runs as by default.
    pub is_default: bool,
}

/// Resolve the Hermes home dir (`~/.hermes`), honoring `HERMES_HOME`.
fn hermes_home() -> Option<PathBuf> {
    if let Ok(h) = std::env::var("HERMES_HOME") {
        return Some(PathBuf::from(h));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".hermes"))
}

/// Extract `default`, `provider`, `base_url` from the `model:` block of a
/// Hermes `config.yaml`. Line-based: find the top-level `model:` key, then read
/// the indented child lines until the indentation returns to column 0.
fn parse_model_block(yaml: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut in_model = false;
    let (mut model, mut provider, mut base_url) = (None, None, None);
    for line in yaml.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if !in_model {
            if trimmed.starts_with("model:") && indent == 0 {
                in_model = true;
            }
            continue;
        }
        // A new top-level key (indent 0, non-empty, not a comment) ends the block.
        if indent == 0 && !trimmed.is_empty() && !trimmed.starts_with('#') {
            break;
        }
        let kv = |k: &str| {
            trimmed
                .strip_prefix(k)
                .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|s| !s.is_empty())
        };
        if let Some(v) = kv("default:") {
            model = Some(v);
        } else if let Some(v) = kv("provider:") {
            provider = Some(v);
        } else if let Some(v) = kv("base_url:") {
            base_url = Some(v);
        }
    }
    (model, provider, base_url)
}

/// One agent built from a config file path. `id`/`is_default` are supplied by
/// the caller; provider/model are parsed from the file (missing file → Nones).
fn agent_from_config(id: &str, path: &PathBuf, is_default: bool) -> AgentInfo {
    let (model, provider, _base_url) = std::fs::read_to_string(path)
        .ok()
        .map(|y| parse_model_block(&y))
        .unwrap_or((None, None, None));
    AgentInfo {
        id: id.to_string(),
        provider,
        model,
        kind: "hermes".to_string(),
        is_default,
    }
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn which_in_path(binary: &str, path_env: &str) -> Option<PathBuf> {
    std::env::split_paths(path_env)
        .map(|dir| dir.join(binary))
        .find(|path| is_executable(path))
}

fn command_version_with_timeout(binary: &Path, timeout: Duration) -> Option<String> {
    let mut child = std::process::Command::new(binary)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let output = child.wait_with_output().ok()?;
                let text = if output.stdout.is_empty() {
                    String::from_utf8_lossy(&output.stderr).to_string()
                } else {
                    String::from_utf8_lossy(&output.stdout).to_string()
                };
                return text
                    .lines()
                    .next()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }
}

fn discover_cli_harnesses(path_env: &str) -> Vec<AgentInfo> {
    let mut out = Vec::new();
    if let Some(claude) = which_in_path("claude", path_env) {
        out.push(AgentInfo {
            id: CLAUDE_CODE_AGENT_ID.to_string(),
            provider: Some(CLAUDE_CODE_AGENT_ID.to_string()),
            model: command_version_with_timeout(&claude, Duration::from_secs(2)),
            kind: CLAUDE_CODE_AGENT_ID.to_string(),
            is_default: false,
        });
    }
    if let Some(codex) = which_in_path("codex", path_env) {
        out.push(AgentInfo {
            id: CODEX_AGENT_ID.to_string(),
            provider: Some("openai-codex".to_string()),
            model: command_version_with_timeout(&codex, Duration::from_secs(2)),
            kind: CODEX_AGENT_ID.to_string(),
            is_default: false,
        });
    }
    out
}

/// Probe the local host's PATH for CLI harnesses (claude, codex), fresh each
/// call. This is the local envoy's job — no process-lifetime cache, so a manual
/// "detect agents" refresh picks up newly-installed CLIs.
fn discover_cli_harnesses_now() -> Vec<AgentInfo> {
    std::env::var_os("PATH")
        .and_then(|p| p.into_string().ok())
        .map(|path| discover_cli_harnesses(&path))
        .unwrap_or_default()
}

/// Discover every agent available on THIS host — the local node's envoy view:
/// the root Hermes profile (as `default`), each `~/.hermes/profiles/<name>/`,
/// and any installed CLI harnesses (claude, codex). Probed fresh (no cache) so
/// a manual refresh reflects installs/uninstalls. This is what the local node
/// reports; a remote envoy runs the equivalent on its own host.
pub fn discover_local_agents() -> Vec<AgentInfo> {
    let Some(home) = hermes_home() else {
        return discover_cli_harnesses_now();
    };
    let mut out = vec![agent_from_config("default", &home.join("config.yaml"), true)];

    if let Ok(entries) = std::fs::read_dir(home.join("profiles")) {
        let mut profiles: Vec<AgentInfo> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let cfg = e.path().join("config.yaml");
                if cfg.exists() {
                    Some(agent_from_config(&name, &cfg, false))
                } else {
                    None
                }
            })
            .collect();
        profiles.sort_by(|a, b| a.id.cmp(&b.id));
        out.extend(profiles);
    }
    out.extend(discover_cli_harnesses_now());
    out
}

/// List all drivable agents. DEPRECATED as fleet truth — this probes the
/// control-plane host directly. The registry (per-node, envoy-reported) is the
/// real source; kept only as a fallback + for the flat model list.
pub fn list_agents() -> Vec<AgentInfo> {
    discover_local_agents()
}

/// Distinct models across all agents, for the model picker. Each entry pairs
/// the model id with the provider it was seen under.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub provider: Option<String>,
}

/// Build the model list from the agents' configured models (deduped by id).
/// When `provider_filter` is `Some`, only models served by that provider are
/// returned — this is what makes the model selector agent-specific (a Codex
/// agent must not be offered Claude Opus, etc.).
pub fn list_models_for(provider_filter: Option<&str>) -> Vec<ModelInfo> {
    let mut seen = std::collections::BTreeMap::new();
    for a in list_agents() {
        if let Some(model) = a.model {
            if let Some(want) = provider_filter {
                if a.provider.as_deref() != Some(want) {
                    continue;
                }
            }
            seen.entry(model.clone()).or_insert(ModelInfo {
                id: model,
                provider: a.provider,
            });
        }
    }
    seen.into_values().collect()
}

/// All models across every agent (deduped). Prefer `list_models_for` with the
/// session's agent provider so the selector stays agent-specific.
pub fn list_models() -> Vec<ModelInfo> {
    list_models_for(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_model_block_extracts_default_provider_base_url() {
        let yaml = "model:\n  default: claude-opus-4-8\n  provider: anthropic\n  base_url: \"\"\nproviders: {}\n";
        let (m, p, b) = parse_model_block(yaml);
        assert_eq!(m.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(p.as_deref(), Some("anthropic"));
        assert_eq!(b, None, "empty base_url is filtered to None");
    }

    #[test]
    fn parse_model_block_stops_at_next_top_level_key() {
        // A `default:` under a LATER top-level key must not leak into the model block.
        let yaml =
            "model:\n  default: gpt-5.4\n  provider: openai-codex\nother:\n  default: NOPE\n";
        let (m, p, _) = parse_model_block(yaml);
        assert_eq!(m.as_deref(), Some("gpt-5.4"));
        assert_eq!(p.as_deref(), Some("openai-codex"));
    }

    #[test]
    fn parse_model_block_handles_base_url_with_value() {
        let yaml = "model:\n  default: gpt-5.5\n  provider: openai-codex\n  base_url: https://chatgpt.com/backend-api/codex\n";
        let (_, _, b) = parse_model_block(yaml);
        assert_eq!(b.as_deref(), Some("https://chatgpt.com/backend-api/codex"));
    }

    #[test]
    fn parse_model_block_missing_block_is_all_none() {
        let (m, p, b) = parse_model_block("providers: {}\nlog_level: info\n");
        assert!(m.is_none() && p.is_none() && b.is_none());
    }

    #[cfg(unix)]
    fn write_stub(dir: &Path, name: &str, body: &str) {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn discover_cli_harnesses_finds_stubbed_claude_and_codex() {
        let tmp = tempfile::tempdir().unwrap();
        write_stub(
            tmp.path(),
            "claude",
            "#!/bin/sh\necho '2.1.195 (Claude Code)'\n",
        );
        write_stub(tmp.path(), "codex", "#!/bin/sh\necho 'codex-cli 0.133.0'\n");

        let agents = discover_cli_harnesses(tmp.path().to_str().unwrap());

        assert!(agents.iter().any(|a| {
            a.id == "claude-code"
                && a.provider.as_deref() == Some("claude-code")
                && a.kind == "claude-code"
                && a.model.as_deref() == Some("2.1.195 (Claude Code)")
                && !a.is_default
        }));
        assert!(agents.iter().any(|a| {
            a.id == "codex"
                && a.provider.as_deref() == Some("openai-codex")
                && a.kind == "codex"
                && a.model.as_deref() == Some("codex-cli 0.133.0")
                && !a.is_default
        }));
    }

    #[cfg(unix)]
    #[test]
    fn discover_cli_harnesses_ignores_non_executable_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("claude"), "#!/bin/sh\necho nope\n").unwrap();
        assert!(discover_cli_harnesses(tmp.path().to_str().unwrap()).is_empty());
    }
}
