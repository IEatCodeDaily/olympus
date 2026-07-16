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

use serde::{Deserialize, Serialize};

use crate::bridge::child::command_for_agent;

const CLAUDE_CODE_AGENT_ID: &str = "claude-code";
const CODEX_AGENT_ID: &str = "codex";

/// Curated model catalog for the Claude Code CLI harness. The CLI accepts
/// `--model` with these slugs; the set is stable per release. (Approach
/// borrowed from t3code/opencode-style tools which ship known harness
/// catalogs instead of probing.)
const CLAUDE_CODE_MODELS: &[&str] = &["claude-opus-4-8", "claude-sonnet-4-6", "claude-haiku-4-5"];

/// Curated model catalog for the Codex CLI harness (`-m/--model`).
const CODEX_MODELS: &[&str] = &["gpt-5.5", "gpt-5.5-codex", "gpt-5.4", "gpt-5.4-mini"];

/// One drivable agent (Hermes profile or local CLI harness) as the UI consumes it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// Agent id passed back in `POST /api/sessions { agent }`.
    pub id: String,
    /// Configured provider (e.g. "anthropic", "openai-codex", "zai").
    pub provider: Option<String>,
    /// Configured default model. NEVER a version string — CLI versions go in
    /// `version`.
    pub model: Option<String>,
    /// Discovered CLI version (CLI harnesses only, e.g. "codex-cli 0.133.0").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Agent harness kind: "hermes", "claude-code", or "codex".
    pub kind: String,
    /// Whether this is the implicit root profile the server runs as by default.
    pub is_default: bool,
    /// Auth readiness for CLI harnesses: Some(true) = credentials found,
    /// Some(false) = installed but logged out ("needs login"), None = not
    /// probed (Hermes profiles carry their own credentials).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready: Option<bool>,
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

/// Parse the `fallback_providers:` block from a Hermes config.yaml. Returns
/// a list of (model, provider) pairs from the fallback list — these are models
/// the provider can serve beyond the default. Used to populate the model picker.
///
/// Handles the standard YAML list-item shape where the first key sits on the
/// dash line:
///   fallback_providers:
///     - model: glm-5v-turbo
///       provider: zai
fn parse_fallback_models(yaml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_fallback = false;
    let (mut cur_model, mut cur_provider): (Option<String>, Option<String>) = (None, None);

    let flush =
        |m: &mut Option<String>, p: &mut Option<String>, out: &mut Vec<(String, String)>| {
            if let (Some(model), Some(provider)) = (m.take(), p.take()) {
                if is_valid_model_id(&model) {
                    out.push((model, provider));
                }
            } else {
                m.take();
                p.take();
            }
        };

    for line in yaml.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if !in_fallback {
            if trimmed.starts_with("fallback_providers:") && indent == 0 {
                in_fallback = true;
            }
            continue;
        }
        // A new top-level key ends the block.
        if indent == 0 && !trimmed.is_empty() && !trimmed.starts_with('#') {
            break;
        }
        // A dash starts a new list entry — flush the previous one, then parse
        // the rest of the dash line (YAML puts the first key on it).
        let content = if let Some(rest) = trimmed.strip_prefix("- ") {
            flush(&mut cur_model, &mut cur_provider, &mut out);
            rest
        } else if trimmed == "-" {
            flush(&mut cur_model, &mut cur_provider, &mut out);
            continue;
        } else {
            trimmed
        };
        let kv = |k: &str| {
            content
                .strip_prefix(k)
                .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|s| !s.is_empty())
        };
        if let Some(v) = kv("model:") {
            cur_model = Some(v);
        } else if let Some(v) = kv("provider:") {
            cur_provider = Some(v);
        }
    }
    flush(&mut cur_model, &mut cur_provider, &mut out);
    out
}

/// Heuristic: a valid model id contains at least one alphanumeric, doesn't
/// start with a digit (version numbers), and doesn't contain spaces with
/// "cli" (version strings like "codex-cli 0.133.0").
fn is_valid_model_id(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(char::is_numeric)
        && !s.contains(" cli ")
        && !s.contains("-cli ")
        && s.chars().any(|c| c.is_alphanumeric())
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
        version: None,
        kind: "hermes".to_string(),
        is_default,
        ready: None,
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

/// Probe whether a CLI harness has stored credentials — binary-exists is not
/// enough (an installed-but-logged-out codex lists as usable, then fails the
/// first message with `Authentication required`). Cheap filesystem checks
/// only; no subprocess, no network.
fn probe_cli_auth(kind: &str) -> Option<bool> {
    let home = std::env::var("HOME").ok()?;
    let home = Path::new(&home);
    match kind {
        // Codex stores ChatGPT/API credentials at ~/.codex/auth.json.
        "codex" => Some(nonempty_file(&home.join(".codex/auth.json"))),
        // Claude Code stores OAuth creds at ~/.claude/.credentials.json;
        // an API key via env also counts.
        "claude-code" => Some(
            nonempty_file(&home.join(".claude/.credentials.json"))
                || std::env::var("ANTHROPIC_API_KEY")
                    .map(|v| !v.is_empty())
                    .unwrap_or(false),
        ),
        _ => None,
    }
}

fn nonempty_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
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

fn discover_cli_harnesses(path_env: &str, claude_adapter: &Path) -> Vec<AgentInfo> {
    let mut out = Vec::new();
    if is_executable(claude_adapter) {
        out.push(AgentInfo {
            id: CLAUDE_CODE_AGENT_ID.to_string(),
            provider: Some(CLAUDE_CODE_AGENT_ID.to_string()),
            // Default model = first entry of the curated catalog; the CLI
            // version string goes in `version`, NOT `model` (it used to leak
            // into the model picker as "codex-cli 0.133.0").
            model: CLAUDE_CODE_MODELS.first().map(|s| s.to_string()),
            version: command_version_with_timeout(claude_adapter, Duration::from_secs(2)),
            kind: CLAUDE_CODE_AGENT_ID.to_string(),
            is_default: false,
            ready: probe_cli_auth(CLAUDE_CODE_AGENT_ID),
        });
    }
    if let Some(codex) = which_in_path("codex", path_env) {
        out.push(AgentInfo {
            id: CODEX_AGENT_ID.to_string(),
            provider: Some("openai-codex".to_string()),
            model: CODEX_MODELS.first().map(|s| s.to_string()),
            version: command_version_with_timeout(&codex, Duration::from_secs(2)),
            kind: CODEX_AGENT_ID.to_string(),
            is_default: false,
            ready: probe_cli_auth(CODEX_AGENT_ID),
        });
    }
    out
}

/// Probe the local host's PATH for CLI harnesses (claude, codex), fresh each
/// call. This is the local envoy's job — no process-lifetime cache, so a manual
/// "detect agents" refresh picks up newly-installed CLIs.
fn discover_cli_harnesses_now() -> Vec<AgentInfo> {
    let claude_adapter = command_for_agent(Some(CLAUDE_CODE_AGENT_ID))
        .into_iter()
        .next()
        .map(PathBuf::from);
    std::env::var_os("PATH")
        .and_then(|p| p.into_string().ok())
        .zip(claude_adapter)
        .map(|(path, adapter)| discover_cli_harnesses(&path, &adapter))
        .unwrap_or_default()
}

fn discover_hermes_profiles(home: &Path, path_env: &str) -> Vec<AgentInfo> {
    if which_in_path("hermes", path_env).is_none() {
        return Vec::new();
    }
    let mut out = vec![agent_from_config(
        "default",
        &home.join("config.yaml"),
        true,
    )];
    if let Ok(entries) = std::fs::read_dir(home.join("profiles")) {
        let mut profiles: Vec<AgentInfo> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let cfg = e.path().join("config.yaml");
                cfg.exists().then(|| agent_from_config(&name, &cfg, false))
            })
            .collect();
        profiles.sort_by(|a, b| a.id.cmp(&b.id));
        out.extend(profiles);
    }
    out
}

/// Discover every agent available on THIS host — the local node's envoy view:
/// the root Hermes profile (as `default`), each `~/.hermes/profiles/<name>/`,
/// and any installed CLI harnesses (claude, codex). Probed fresh (no cache) so
/// a manual refresh reflects installs/uninstalls. This is what the local node
/// reports; a remote envoy runs the equivalent on its own host.
pub fn discover_local_agents() -> Vec<AgentInfo> {
    let path = std::env::var("PATH").unwrap_or_default();
    let mut out = hermes_home()
        .map(|home| discover_hermes_profiles(&home, &path))
        .unwrap_or_default();
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
///
/// Sources BOTH the `model.default` and `fallback_providers` blocks from each
/// agent's config.yaml, so the picker shows all models the provider can serve —
/// not just the one configured as default.
pub fn list_models_for(provider_filter: Option<&str>) -> Vec<ModelInfo> {
    let mut seen = std::collections::BTreeMap::new();

    // Curated CLI-harness catalogs (claude-code / codex). These CLIs accept a
    // fixed set of --model slugs per release; there is nothing to probe.
    let mut add_catalog = |provider: &str, catalog: &[&str]| {
        if let Some(want) = provider_filter {
            if want != provider {
                return;
            }
        }
        for m in catalog {
            seen.entry(m.to_string()).or_insert(ModelInfo {
                id: m.to_string(),
                provider: Some(provider.to_string()),
            });
        }
    };
    add_catalog(CLAUDE_CODE_AGENT_ID, CLAUDE_CODE_MODELS);
    add_catalog("openai-codex", CODEX_MODELS);

    for a in list_agents() {
        // Include the default model
        if let Some(ref model) = a.model {
            if is_valid_model_id(model) {
                if let Some(want) = provider_filter {
                    if a.provider.as_deref() != Some(want) {
                        continue;
                    }
                }
                seen.entry(model.clone()).or_insert(ModelInfo {
                    id: model.clone(),
                    provider: a.provider.clone(),
                });
            }
        }
    }
    // Also parse fallback_providers from the config files (these are models the
    // provider serves beyond the default — the user can switch to them)
    if let Some(home) = hermes_home() {
        let configs = std::iter::once(home.join("config.yaml"))
            .chain(
                std::fs::read_dir(home.join("profiles"))
                    .ok()
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.path().join("config.yaml")),
            )
            .filter(|p| p.exists());
        for cfg_path in configs {
            if let Ok(yaml) = std::fs::read_to_string(&cfg_path) {
                for (model, provider) in parse_fallback_models(&yaml) {
                    if let Some(want) = provider_filter {
                        if provider != want {
                            continue;
                        }
                    }
                    seen.entry(model.clone()).or_insert(ModelInfo {
                        id: model,
                        provider: Some(provider),
                    });
                }
            }
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
    fn parse_fallback_models_reads_dash_line_key_shape() {
        // The REAL Hermes config shape: first key on the dash line.
        let yaml = "model:\n  default: glm-5.2\n  provider: zai\nfallback_providers:\n  - model: glm-5v-turbo\n    provider: zai\n  - model: gpt-5.5\n    provider: openai-codex\ncredential_pool_strategies:\n  anthropic: fill_first\n";
        let models = parse_fallback_models(yaml);
        assert_eq!(
            models,
            vec![
                ("glm-5v-turbo".to_string(), "zai".to_string()),
                ("gpt-5.5".to_string(), "openai-codex".to_string()),
            ]
        );
    }

    #[test]
    fn parse_fallback_models_filters_version_strings() {
        let yaml =
            "fallback_providers:\n  - model: codex-cli 0.133.0\n    provider: openai-codex\n";
        assert!(parse_fallback_models(yaml).is_empty());
    }

    #[test]
    fn is_valid_model_id_rejects_versions() {
        assert!(is_valid_model_id("glm-5.2"));
        assert!(is_valid_model_id("claude-sonnet-4-6"));
        assert!(!is_valid_model_id("0.133.0"));
        assert!(!is_valid_model_id("codex-cli 0.133.0"));
        assert!(!is_valid_model_id(""));
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
    fn hermes_profiles_require_a_runnable_hermes_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join(".hermes");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(home.join("config.yaml"), "model:\n  default: test\n").unwrap();

        assert!(discover_hermes_profiles(&home, tmp.path().to_str().unwrap()).is_empty());

        write_stub(tmp.path(), "hermes", "#!/bin/sh\nexit 0\n");
        assert_eq!(
            discover_hermes_profiles(&home, tmp.path().to_str().unwrap())[0].id,
            "default"
        );
    }

    #[cfg(unix)]
    #[test]
    fn discover_cli_harnesses_finds_runtime_adapter_and_codex() {
        let tmp = tempfile::tempdir().unwrap();
        write_stub(
            tmp.path(),
            "claude-agent-acp",
            "#!/bin/sh\necho '2.1.195 (Claude Code)'\n",
        );
        write_stub(tmp.path(), "codex", "#!/bin/sh\necho 'codex-cli 0.133.0'\n");

        let agents = discover_cli_harnesses(
            tmp.path().to_str().unwrap(),
            &tmp.path().join("claude-agent-acp"),
        );

        assert!(agents.iter().any(|a| {
            a.id == "claude-code"
                && a.provider.as_deref() == Some("claude-code")
                && a.kind == "claude-code"
                // model = curated catalog default; the CLI version string goes
                // to `version` (it used to leak into the model picker).
                && a.model.as_deref() == CLAUDE_CODE_MODELS.first().copied()
                && a.version.as_deref() == Some("2.1.195 (Claude Code)")
                && !a.is_default
        }));
        assert!(agents.iter().any(|a| {
            a.id == "codex"
                && a.provider.as_deref() == Some("openai-codex")
                && a.kind == "codex"
                && a.model.as_deref() == CODEX_MODELS.first().copied()
                && a.version.as_deref() == Some("codex-cli 0.133.0")
                && !a.is_default
        }));
    }

    #[cfg(unix)]
    #[test]
    fn discover_cli_harnesses_ignores_non_executable_files() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = tmp.path().join("claude-agent-acp");
        std::fs::write(&adapter, "#!/bin/sh\necho nope\n").unwrap();
        assert!(discover_cli_harnesses(tmp.path().to_str().unwrap(), &adapter).is_empty());
    }
}
