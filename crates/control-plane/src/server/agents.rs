//! Agent discovery — lists the Hermes "agents" (profiles) Olympus can drive,
//! with their configured provider + model, so the UI can offer a real
//! provider/model picker instead of a hardcoded list.
//!
//! An "agent" in Hermes is a profile: `~/.hermes/profiles/<name>/config.yaml`
//! plus the implicit root profile (`~/.hermes/config.yaml`, exposed as the
//! `default` agent). We parse the small `model:` block (default/provider/
//! base_url) with a line scanner rather than pulling in a YAML dependency —
//! the block shape is stable and this avoids the deprecated serde_yaml.

use std::path::PathBuf;

use serde::Serialize;

/// One drivable agent (Hermes profile) as the UI consumes it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// Profile id passed to `hermes -p <id> acp` (or "default" for the root).
    pub id: String,
    /// Configured provider (e.g. "anthropic", "openai-codex", "zai").
    pub provider: Option<String>,
    /// Configured default model (e.g. "claude-opus-4-8", "gpt-5.4").
    pub model: Option<String>,
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
        is_default,
    }
}

/// List all drivable agents: the root profile (as `default`) plus every
/// `~/.hermes/profiles/<name>/`. Sorted with `default` first, then by id.
pub fn list_agents() -> Vec<AgentInfo> {
    let Some(home) = hermes_home() else {
        return Vec::new();
    };
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
    out
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
pub fn list_models() -> Vec<ModelInfo> {
    let mut seen = std::collections::BTreeMap::new();
    for a in list_agents() {
        if let Some(model) = a.model {
            seen.entry(model.clone()).or_insert(ModelInfo {
                id: model,
                provider: a.provider,
            });
        }
    }
    seen.into_values().collect()
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
}
