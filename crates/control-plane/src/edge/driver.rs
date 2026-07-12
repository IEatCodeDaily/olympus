use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthPolicy {
    Public,
    SessionScoped,
    AppGrant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPort {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub id: String,
    pub path_prefix: String,
    pub upstream: Option<HostPort>,
    pub artifact_root: Option<PathBuf>,
    pub auth_policy: AuthPolicy,
    pub websocket: bool,
}

pub trait EdgeDriver: Send + Sync {
    fn apply(&self, desired: &[Route]) -> Result<()>;
    fn healthy(&self) -> bool;
}
