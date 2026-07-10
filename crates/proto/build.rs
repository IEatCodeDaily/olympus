//! Build script — embeds the git hash and build timestamp into the crate so
//! [`BuildVersion::current`] can report a real build identity (ADR 0008 §1:
//! `version: {semver, gitHash, builtAt}` is what drain/evict decisions key on).
//!
//! Falls back to `"unknown"` when git is absent or the tree is not a repo
//! (e.g. a source tarball build) — never fails the build.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=OLYMPUS_GIT_HASH={git_hash}");

    let built_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=OLYMPUS_BUILT_AT={built_at}");

    // Watching .git/HEAD alone is insufficient on a branch: its contents stay
    // `ref: refs/heads/<name>` while the referenced file moves on every commit.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    if let Ok(output) = Command::new("git")
        .args(["symbolic-ref", "-q", "HEAD"])
        .output()
    {
        if output.status.success() {
            if let Ok(reference) = String::from_utf8(output.stdout) {
                let reference = reference.trim();
                if !reference.is_empty() {
                    if let Ok(path) = Command::new("git")
                        .args(["rev-parse", "--git-path", reference])
                        .output()
                    {
                        if path.status.success() {
                            if let Ok(path) = String::from_utf8(path.stdout) {
                                println!("cargo:rerun-if-changed={}", path.trim());
                            }
                        }
                    }
                }
            }
        }
    }
    println!("cargo:rerun-if-changed=../../.git/packed-refs");
    println!("cargo:rerun-if-changed=build.rs");
}
