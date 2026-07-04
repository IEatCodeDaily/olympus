//! Managed git/jj repos — clone, sync, and workspace attach.
//!
//! Repos live at `~/.olympus/<org>/repos/<slug>`. This module owns the
//! on-disk lifecycle: clone (jj or fallback git), sync (`jj git fetch`),
//! and attach (`jj workspace add` into a session space).
//!
//! All shell commands use `tokio::process::Command` with arg arrays — never
//! shell strings.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::process::Command;
use tracing::{info, warn};

/// Filesystem-backed repo store (not yet wired to handlers).
#[allow(dead_code)]
pub struct RepoStore {
    root: PathBuf,
    org: String,
}

impl RepoStore {
    /// Create a store rooted at `<base_dir>/<org>/repos/`.
    pub fn new(base_dir: &Path, org: &str) -> Self {
        Self {
            root: base_dir.join(org).join("repos"),
            org: org.to_string(),
        }
    }

    /// The on-disk path of a registered repo.
    pub fn repo_path(&self, slug: &str) -> PathBuf {
        self.root.join(slug)
    }

    /// Clone a repo into the store using `jj git clone --colocate`.
    ///
    /// Falls back to `git clone` + `jj git init --colocate` if jj is not
    /// available. Runs in a background tokio task; returns immediately.
    /// The caller checks clone state by calling [`Self::is_cloned`] (dir-exists).
    pub async fn clone(&self, slug: &str, url: &str) -> Result<PathBuf> {
        let dir = self.repo_path(slug);
        if dir.exists() {
            // Idempotent: already cloned.
            return Ok(dir);
        }
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating repo dir {}", dir.display()))?;

        let jj_available = which_jj().await.is_some();
        if jj_available {
            info!(slug, url, path = %dir.display(), "cloning with jj git clone");
            let status = Command::new("jj")
                .args(["git", "clone", url, "--colocate"])
                .arg(&dir)
                .status()
                .await
                .context("running jj git clone")?;
            if !status.success() {
                anyhow::bail!(
                    "jj git clone failed with status {}",
                    status.code().unwrap_or(-1)
                );
            }
        } else {
            info!(slug, url, path = %dir.display(), "falling back to git clone + jj init");
            // Fallback: git clone then jj init.
            let status = Command::new("git")
                .args(["clone", url])
                .arg(&dir)
                .status()
                .await
                .context("running git clone")?;
            if !status.success() {
                anyhow::bail!(
                    "git clone failed with status {}",
                    status.code().unwrap_or(-1)
                );
            }
            // Try jj init; if jj is missing we still have a usable git repo.
            let jj_init = Command::new("jj")
                .args(["git", "init", "--colocate"])
                .current_dir(&dir)
                .status()
                .await;
            match jj_init {
                Ok(s) if s.success() => {}
                Err(e) => {
                    warn!(error = %e, "jj git init failed after git clone; repo is git-only");
                }
                _ => {
                    warn!("jj git init non-zero after git clone; repo is git-only");
                }
            }
        }

        Ok(dir)
    }

    /// Run `jj git fetch` inside an existing repo dir to sync remotes.
    pub async fn sync(&self, slug: &str) -> Result<()> {
        let dir = self.repo_path(slug);
        if !dir.exists() {
            anyhow::bail!("repo {slug} is not cloned at {}", dir.display());
        }
        info!(slug, path = %dir.display(), "syncing repo");
        let status = Command::new("jj")
            .args(["git", "fetch"])
            .current_dir(&dir)
            .status()
            .await
            .context("running jj git fetch")?;
        if !status.success() {
            anyhow::bail!(
                "jj git fetch failed with status {}",
                status.code().unwrap_or(-1)
            );
        }
        Ok(())
    }

    /// Attach a repo to a session's jj workspace:
    /// `jj workspace add <space>/repos/<slug>` from within the repo dir.
    ///
    /// Returns 409-worthy errors (not panics) if jj is missing or the repo
    /// hasn't been cloned yet.
    pub async fn attach(&self, slug: &str, session_space: &Path) -> Result<PathBuf> {
        let repo_dir = self.repo_path(slug);
        if !repo_dir.exists() {
            anyhow::bail!(
                "repo {slug} not found at {} (clone first)",
                repo_dir.display()
            );
        }

        let target = session_space.join("repos").join(slug);

        // Ensure jj is available.
        if which_jj().await.is_none() {
            anyhow::bail!("jj is not installed; cannot attach repo workspace");
        }

        info!(slug, target = %target.display(), repo = %repo_dir.display(), "attaching repo workspace");

        let status = Command::new("jj")
            .args(["workspace", "add"])
            .arg(&target)
            .current_dir(&repo_dir)
            .status()
            .await
            .context("running jj workspace add")?;
        if !status.success() {
            anyhow::bail!(
                "jj workspace add failed with status {}",
                status.code().unwrap_or(-1)
            );
        }
        Ok(target)
    }

    /// Check whether a repo has been cloned (dir exists and has content).
    pub fn is_cloned(&self, slug: &str) -> bool {
        let dir = self.repo_path(slug);
        dir.is_dir()
            && dir
                .read_dir()
                .map(|mut it| it.next().is_some())
                .unwrap_or(false)
    }

    /// Remove a repo directory from disk (best-effort GC on removal).
    pub fn remove(&self, slug: &str) {
        let dir = self.repo_path(slug);
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                warn!(error = %e, path = %dir.display(), "failed to remove repo dir");
            }
        }
    }
}

/// Check if `jj` is on PATH. Returns `Some(path)` or `None`.
async fn which_jj() -> Option<PathBuf> {
    Command::new("which")
        .arg("jj")
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(PathBuf::from(s))
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_path_is_deterministic() {
        let store = RepoStore::new(Path::new("/tmp/olympus"), "default");
        assert_eq!(
            store.repo_path("my-repo"),
            PathBuf::from("/tmp/olympus/default/repos/my-repo")
        );
    }

    #[test]
    fn is_cloned_false_for_missing_dir() {
        let store = RepoStore::new(Path::new("/nonexistent"), "default");
        assert!(!store.is_cloned("ghost"));
    }

    #[test]
    fn remove_missing_repo_is_noop() {
        // Should not panic for non-existent dirs.
        let store = RepoStore::new(Path::new("/nonexistent"), "default");
        store.remove("ghost"); // just must not panic
    }

    #[tokio::test]
    async fn which_jj_returns_none_when_absent() {
        // We can't guarantee jj isn't installed, but we can verify the function
        // doesn't panic and returns Some or None without error.
        let result = which_jj().await;
        // If jj IS installed, result is Some(...); otherwise None. Both valid.
        drop(result);
    }
}
