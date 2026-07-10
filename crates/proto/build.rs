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

    // Re-run when HEAD moves so the embedded hash tracks the checkout.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=build.rs");
}
