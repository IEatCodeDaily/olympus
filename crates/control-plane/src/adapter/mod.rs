//! Setup adapter — resolves a declared setup into a harness-specific
//! materialization (ADR 0006 §9).
//!
//! The chain: declaration (slug lists) → registry (slug→definition) →
//! adapter (render into harness config) → spawn (point runtime at it).
//!
//! Each adapter renders INTO the session space — never into a shared profile.
//! This avoids the Hermes Studio cross-contamination original sin by
//! construction (ADR 0002 §1.1).

pub mod claude_code;
pub mod codex;
pub mod hermes;

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::views::{RegistryEntry, RegistryView};

/// Which agent harness is locked to this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Hermes,
    ClaudeCode,
    Codex,
}

impl AgentKind {
    /// Resolve an agent name string (as stored on the session) into a kind.
    /// Matching is case-insensitive and checks for common substrings.
    /// Unknown/empty defaults to Hermes (the original harness).
    pub fn from_agent_str(agent: &str) -> Self {
        let lower = agent.to_ascii_lowercase();
        if lower.contains("claude") {
            Self::ClaudeCode
        } else if lower.contains("codex") {
            Self::Codex
        } else {
            Self::Hermes
        }
    }
}

/// How to merge Olympus's declared setup with the harness's existing config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    /// Olympus additions layer on top of the harness's profile defaults.
    /// (Hermes default.)
    Union,
    /// Olympus's declaration fully replaces the harness config for this session.
    /// (Configurable for Claude Code / Codex.)
    Override,
}

/// What a harness can do with each setup category. Drives the drop-with-
/// warning vs fallback-to-context decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// The harness has a native mechanism for this category.
    Native,
    /// No native support, but the content degrades to prose appended to the
    /// context file (CLAUDE.md / AGENTS.md).
    FallbackToContext,
    /// Neither — the item is dropped with a surfaced warning.
    Unsupported,
}

/// The per-harness capability matrix. Drives materialization behavior.
#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    pub skills: Support,
    pub mcp: Support,
    pub hooks: Support,
    pub plugins: Support,
}

/// The env vars + args the runtime factory applies when spawning the agent,
/// after the adapter has written config into the session space.
#[derive(Debug, Clone, Default)]
pub struct SpawnOverlay {
    /// Environment variables to set on the child process.
    pub env: Vec<(String, String)>,
    /// Extra arguments to append to the command.
    pub args: Vec<String>,
    /// MCP servers resolved from the registry — passed to the harness's
    /// session/new (Hermes ACP) or written to config (Claude Code .mcp.json,
    /// Codex config.toml). Format: the harness's native MCP server JSON.
    pub mcp_servers: Vec<serde_json::Value>,
    /// Warnings surfaced during materialization (e.g. "declared skill X but
    /// harness doesn't support skills natively — appended to context").
    pub warnings: Vec<String>,
}

/// A setup category's slug list resolved to concrete definitions from the
/// registry. `unresolved` holds slugs that aren't in the registry (so the
/// adapter can warn).
#[derive(Debug, Clone, Default)]
pub struct ResolvedCategory {
    pub resolved: Vec<RegistryEntry>,
    pub unresolved: Vec<String>,
}

/// The fully resolved setup for a session — all four categories resolved from
/// the registry, ready for the adapter to render.
#[derive(Debug, Clone, Default)]
pub struct ResolvedSetup {
    pub skills: ResolvedCategory,
    pub mcp: ResolvedCategory,
    pub plugins: ResolvedCategory,
    pub hooks: ResolvedCategory,
}

impl ResolvedSetup {
    /// Resolve a declared setup (slug lists) against the registry, producing
    /// the concrete definitions the adapter needs. Each category resolves to
    /// (found definitions, missing slugs).
    pub fn from_registry(
        registry: &RegistryView,
        skills: &[String],
        mcp: &[String],
        plugins: &[String],
        hooks: &[String],
    ) -> Self {
        let (s_skills, m_skills) = registry.resolve_batch("skill", skills);
        let (s_mcp, m_mcp) = registry.resolve_batch("mcp", mcp);
        let (s_plugins, m_plugins) = registry.resolve_batch("plugin", plugins);
        let (s_hooks, m_hooks) = registry.resolve_batch("hook", hooks);
        Self {
            skills: ResolvedCategory {
                resolved: s_skills.into_iter().cloned().collect(),
                unresolved: m_skills,
            },
            mcp: ResolvedCategory {
                resolved: s_mcp.into_iter().cloned().collect(),
                unresolved: m_mcp,
            },
            plugins: ResolvedCategory {
                resolved: s_plugins.into_iter().cloned().collect(),
                unresolved: m_plugins,
            },
            hooks: ResolvedCategory {
                resolved: s_hooks.into_iter().cloned().collect(),
                unresolved: m_hooks,
            },
        }
    }

    /// Were any slugs unresolved (not in the registry)?
    pub fn has_unresolved(&self) -> bool {
        !self.skills.unresolved.is_empty()
            || !self.mcp.unresolved.is_empty()
            || !self.plugins.unresolved.is_empty()
            || !self.hooks.unresolved.is_empty()
    }
}

/// The setup adapter trait — renders a resolved setup into a harness-specific
/// materialization. Each impl writes config INTO the session space and returns
/// the spawn overlay (env/args/mcpServers) the runtime factory applies.
pub trait SetupAdapter: Send + Sync {
    fn agent_kind(&self) -> AgentKind;
    fn capabilities(&self) -> Capabilities;

    /// Render the resolved setup into the session space + return the spawn
    /// overlay. Called at session creation (locked harness) and at handover.
    /// Never mutates a shared profile — all output goes into `space`.
    fn materialize(
        &self,
        resolved: &ResolvedSetup,
        space: &Path,
        mode: MergeMode,
    ) -> Result<SpawnOverlay>;
}

/// Select the concrete adapter for a given agent kind.
pub fn for_kind(kind: AgentKind) -> Box<dyn SetupAdapter> {
    match kind {
        AgentKind::Hermes => Box::new(hermes::HermesAdapter),
        AgentKind::ClaudeCode => Box::new(claude_code::ClaudeCodeAdapter),
        AgentKind::Codex => Box::new(codex::CodexAdapter),
    }
}
