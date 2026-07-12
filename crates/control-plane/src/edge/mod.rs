pub mod caddy;
pub mod driver;

use std::path::Path;
use std::sync::{Arc, RwLock};

use anyhow::Result;
pub use driver::{AuthPolicy, EdgeDriver, HostPort, Route};

#[derive(Clone)]
pub struct EdgeManager {
    driver: Arc<dyn EdgeDriver>,
    desired: Arc<RwLock<Vec<Route>>>,
}

impl EdgeManager {
    pub fn new(driver: Arc<dyn EdgeDriver>) -> Self {
        Self {
            driver,
            desired: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn healthy(&self) -> bool {
        self.driver.healthy()
    }

    pub fn routes(&self) -> Vec<Route> {
        self.desired
            .read()
            .expect("edge route lock poisoned")
            .clone()
    }

    pub fn route(&self, id: &str) -> Option<Route> {
        self.desired
            .read()
            .expect("edge route lock poisoned")
            .iter()
            .find(|route| route.id == id)
            .cloned()
    }

    pub fn upsert(&self, route: Route) -> Result<()> {
        if let Some(root) = &route.artifact_root {
            validate_artifact_root(root)?;
        }
        let mut desired = self.desired.write().expect("edge route lock poisoned");
        let mut next = desired.clone();
        if let Some(existing) = next.iter_mut().find(|candidate| candidate.id == route.id) {
            *existing = route;
        } else {
            next.push(route);
        }
        self.driver.apply(&next)?;
        *desired = next;
        Ok(())
    }

    pub fn remove(&self, id: &str) -> Result<bool> {
        let mut desired = self.desired.write().expect("edge route lock poisoned");
        let mut next = desired.clone();
        let before = next.len();
        next.retain(|route| route.id != id);
        if next.len() == before {
            return Ok(false);
        }
        self.driver.apply(&next)?;
        *desired = next;
        Ok(true)
    }

    pub fn converge(&self) -> Result<()> {
        self.driver.apply(&self.routes())
    }
}

/// Caddy follows symlinks inside a file-server root. Keep artifact routes safe
/// by refusing any root whose existing path tree contains one.
pub fn validate_artifact_root(root: &Path) -> Result<()> {
    anyhow::ensure!(root.is_absolute(), "artifact root must be absolute");
    let metadata = std::fs::symlink_metadata(root).map_err(|error| {
        anyhow::anyhow!("artifact root {} is unavailable: {error}", root.display())
    })?;
    anyhow::ensure!(metadata.is_dir(), "artifact root must be a directory");
    anyhow::ensure!(
        !metadata.file_type().is_symlink(),
        "artifact root contains a symlink"
    );

    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let metadata = std::fs::symlink_metadata(entry.path())?;
            anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "artifact root contains a symlink: {}",
                entry.path().display()
            );
            if metadata.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub struct MemoryDriver {
    pub healthy: bool,
    pub applied: std::sync::Mutex<Vec<Vec<Route>>>,
}

#[cfg(test)]
impl MemoryDriver {
    pub fn available() -> Arc<Self> {
        Arc::new(Self {
            healthy: true,
            applied: std::sync::Mutex::new(Vec::new()),
        })
    }
}

#[cfg(test)]
impl EdgeDriver for MemoryDriver {
    fn apply(&self, desired: &[Route]) -> Result<()> {
        anyhow::ensure!(self.healthy, "edge unavailable");
        self.applied.lock().unwrap().push(desired.to_vec());
        Ok(())
    }
    fn healthy(&self) -> bool {
        self.healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn registration_refuses_symlink_inside_artifact_root() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("artifacts");
        std::fs::create_dir(&root).unwrap();
        symlink(temporary.path(), root.join("escape")).unwrap();
        let edge = EdgeManager::new(MemoryDriver::available());
        let error = edge
            .upsert(Route {
                id: "unsafe-static".into(),
                path_prefix: "/artifacts/org/unsafe/".into(),
                upstream: None,
                artifact_root: Some(root),
                auth_policy: AuthPolicy::Public,
                websocket: false,
            })
            .unwrap_err();
        assert!(error.to_string().contains("symlink"));
    }
}
