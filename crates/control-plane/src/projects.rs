//! Project filesystem storage — directory, manifest, and session-space symlink.
//!
//! A project lives at `~/.olympus/<org>/projects/<project_id>/` with a
//! `project.json` manifest.  On `attach`, a symlink `<session_space>/project`
//! is created (or replaced) pointing to the project directory.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

/// Project manifest as stored on disk.
#[derive(Debug, Clone)]
pub struct ProjectManifest {
    pub project_id: String,
    pub name: String,
    pub vaults: Vec<String>,
    pub repos: Vec<String>,
    pub boards: Vec<String>,
    pub created_at: f64,
}

/// Manages the `~/.olympus/<org>/projects/` directory tree.
#[derive(Debug, Clone)]
pub struct ProjectStore {
    /// Absolute path to `~/.olympus/<org>/projects/`.
    root: PathBuf,
}

impl ProjectStore {
    /// Create a store rooted at `org_root/projects/`.
    pub fn new(org_root: impl Into<PathBuf>) -> Self {
        Self {
            root: org_root.into().join("projects"),
        }
    }

    /// Path to a specific project directory.
    pub fn project_dir(&self, project_id: &str) -> PathBuf {
        self.root.join(project_id)
    }

    /// Ensure the projects root exists.
    fn ensure_root(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("creating projects root {}", self.root.display()))
    }

    /// Create the on-disk directory + manifest for a new project.
    pub fn create(&self, project_id: &str, name: &str, created_at: f64) -> Result<()> {
        self.ensure_root()?;
        let dir = self.project_dir(project_id);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating project dir {}", dir.display()))?;
        let manifest = json!({
            "project_id": project_id,
            "name": name,
            "vaults": [],
            "repos": [],
            "boards": [],
            "created_at": created_at,
        });
        write_manifest(&dir, &manifest)?;
        Ok(())
    }

    /// Update the manifest on disk with new field values. `None` = unchanged.
    pub fn update(
        &self,
        project_id: &str,
        name: Option<&str>,
        vaults: Option<&[String]>,
        repos: Option<&[String]>,
        boards: Option<&[String]>,
    ) -> Result<()> {
        let dir = self.project_dir(project_id);
        if !dir.is_dir() {
            // Silently skip — project dir may not exist in test/import scenarios.
            return Ok(());
        }
        let mut manifest = read_manifest(&dir)?;
        if let Some(n) = name {
            manifest["name"] = json!(n);
        }
        if let Some(v) = vaults {
            manifest["vaults"] = json!(v);
        }
        if let Some(r) = repos {
            manifest["repos"] = json!(r);
        }
        if let Some(b) = boards {
            manifest["boards"] = json!(b);
        }
        write_manifest(&dir, &manifest)?;
        Ok(())
    }

    /// Create (or replace) a symlink `<session_space>/project` → project dir.
    ///
    /// If `session_space` is `None`, does nothing (no space configured).
    pub fn attach_symlink(&self, project_id: &str, session_space: Option<&str>) -> Result<()> {
        let Some(space) = session_space else {
            return Ok(());
        };
        let project_dir = self.project_dir(project_id);
        let link_path = PathBuf::from(space).join("project");
        // Remove existing symlink (or file) if present.
        if link_path.exists() || link_path.symlink_metadata().is_ok() {
            std::fs::remove_file(&link_path).with_context(|| {
                format!("removing existing project symlink {}", link_path.display())
            })?;
        }
        std::os::unix::fs::symlink(&project_dir, &link_path).with_context(|| {
            format!(
                "creating project symlink {} → {}",
                link_path.display(),
                project_dir.display()
            )
        })?;
        Ok(())
    }

    /// Read and return the manifest for a project, or `None` if not found.
    pub fn read(&self, project_id: &str) -> Result<Option<ProjectManifest>> {
        let dir = self.project_dir(project_id);
        if !dir.is_dir() {
            return Ok(None);
        }
        let v = read_manifest(&dir)?;
        Ok(Some(manifest_from_value(project_id, v)?))
    }

    /// Validate a project id — must be a single path component, no slashes.
    pub fn validate_id(id: &str) -> bool {
        !id.is_empty()
            && !id.contains('/')
            && !id.contains('\\')
            && id != "."
            && id != ".."
            && Path::new(id).components().count() == 1
    }
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("project.json")
}

fn read_manifest(dir: &Path) -> Result<Value> {
    let path = manifest_path(dir);
    if !path.exists() {
        bail!("project manifest not found at {}", path.display());
    }
    let bytes =
        std::fs::read(&path).with_context(|| format!("reading manifest {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing manifest {}", path.display()))
}

fn write_manifest(dir: &Path, value: &Value) -> Result<()> {
    let path = manifest_path(dir);
    let pretty = serde_json::to_string_pretty(value).context("serializing manifest")?;
    std::fs::write(&path, pretty).with_context(|| format!("writing manifest {}", path.display()))
}

fn manifest_from_value(project_id: &str, v: Value) -> Result<ProjectManifest> {
    fn strings(v: &Value, key: &str) -> Vec<String> {
        v.get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(ToOwned::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }
    Ok(ProjectManifest {
        project_id: project_id.to_string(),
        name: v
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        vaults: strings(&v, "vaults"),
        repos: strings(&v, "repos"),
        boards: strings(&v, "boards"),
        created_at: v.get("created_at").and_then(Value::as_f64).unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_reads_back_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path());
        store.create("proj-1", "My Project", 1000.0).unwrap();

        let m = store.read("proj-1").unwrap().unwrap();
        assert_eq!(m.name, "My Project");
        assert_eq!(m.created_at, 1000.0);
        assert!(m.vaults.is_empty());
    }

    #[test]
    fn update_patches_manifest_fields() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path());
        store.create("proj-1", "Alpha", 1.0).unwrap();
        store
            .update(
                "proj-1",
                Some("Alpha-2"),
                Some(&["vault-a".into()]),
                Some(&["repo-x".into()]),
                None,
            )
            .unwrap();

        let m = store.read("proj-1").unwrap().unwrap();
        assert_eq!(m.name, "Alpha-2");
        assert_eq!(m.vaults, vec!["vault-a"]);
        assert_eq!(m.repos, vec!["repo-x"]);
        assert!(m.boards.is_empty());
    }

    #[test]
    fn read_unknown_project_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path());
        assert!(store.read("ghost").unwrap().is_none());
    }

    #[test]
    fn attach_symlink_creates_link() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path());
        store.create("proj-1", "P", 1.0).unwrap();

        // Create a fake session space.
        let space = dir.path().join("session-space");
        std::fs::create_dir_all(&space).unwrap();

        store
            .attach_symlink("proj-1", Some(space.to_str().unwrap()))
            .unwrap();

        let link = space.join("project");
        assert!(link.symlink_metadata().is_ok(), "symlink should exist");
        let target = std::fs::read_link(&link).unwrap();
        assert_eq!(target, store.project_dir("proj-1"));
    }

    #[test]
    fn attach_symlink_replaces_existing() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path());
        store.create("proj-1", "P1", 1.0).unwrap();
        store.create("proj-2", "P2", 2.0).unwrap();

        let space = dir.path().join("session-space");
        std::fs::create_dir_all(&space).unwrap();

        store
            .attach_symlink("proj-1", Some(space.to_str().unwrap()))
            .unwrap();
        store
            .attach_symlink("proj-2", Some(space.to_str().unwrap()))
            .unwrap();

        let link = space.join("project");
        let target = std::fs::read_link(&link).unwrap();
        assert_eq!(target, store.project_dir("proj-2"));
    }
}
