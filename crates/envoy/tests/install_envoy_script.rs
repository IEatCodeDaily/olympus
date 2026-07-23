use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn fake_bin(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, format!("#!/usr/bin/env bash\n{body}\n")).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
}

fn run_install(home: &Path, args: &[&str]) -> std::process::Output {
    let bin = home.join("fake-bin");
    fs::create_dir_all(&bin).unwrap();
    fake_bin(
        &bin,
        "uname",
        r#"case "$1" in -s) echo Linux;; -m) echo x86_64;; *) /usr/bin/uname "$@";; esac"#,
    );
    fake_bin(&bin, "cargo", "exit 0");
    fake_bin(&bin, "git", r#"echo testhash0000"#);
    fake_bin(&bin, "hermes", "exit 0");
    fake_bin(&bin, "systemctl", "exit 1");
    fake_bin(&bin, "loginctl", "echo Linger=yes");
    fake_bin(&bin, "curl", "exit 22");

    Command::new(repo_root().join("scripts/install-envoy.sh"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("xdg-config"))
        .env("XDG_STATE_HOME", home.join("xdg-state"))
        .env("XDG_RUNTIME_DIR", home.join("xdg-runtime"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap()
}

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn user_tier_uses_xdg_paths_and_no_sudo_escalation() {
    let home = tempfile::tempdir().unwrap();
    let output = run_install(
        home.path(),
        &[
            "--tier",
            "user",
            "--hall",
            "uds:/tmp/hall.sock",
            "--dry-run",
        ],
    );
    assert!(output.status.success(), "{}", combined(&output));
    let text = combined(&output);
    assert!(text.contains("tier:  user"), "{text}");
    assert!(
        text.contains("xdg-state/olympus/bin/olympus-envoy"),
        "{text}"
    );
    assert!(
        text.contains("xdg-config/systemd/user/olympus-envoy@.service"),
        "{text}"
    );
    assert!(
        !text.contains("sudo"),
        "user tier must not suggest sudo: {text}"
    );
}

#[test]
fn system_tier_unit_has_service_account_and_capability_ceiling() {
    let home = tempfile::tempdir().unwrap();
    let output = run_install(
        home.path(),
        &[
            "--tier",
            "system",
            "--hall",
            "uds:/tmp/hall.sock",
            "--dry-run",
        ],
    );
    assert!(output.status.success(), "{}", combined(&output));
    let text = combined(&output);
    assert!(text.contains("tier:  system"), "{text}");
    assert!(
        text.contains("/etc/systemd/system/olympus-envoy.service"),
        "{text}"
    );
    assert!(text.contains("User=olympus-envoy"), "{text}");
    assert!(text.contains("CapabilityBoundingSet="), "{text}");
    assert!(text.contains("NoNewPrivileges=yes"), "{text}");
}

#[test]
fn duplicate_tier_is_refused_without_repair_migration_or_uninstall() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("xdg-config/systemd/user")).unwrap();
    fs::write(
        home.path()
            .join("xdg-config/systemd/user/olympus-envoy@.service"),
        "installed",
    )
    .unwrap();

    let output = run_install(
        home.path(),
        &[
            "--tier",
            "system",
            "--hall",
            "uds:/tmp/hall.sock",
            "--dry-run",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    let text = combined(&output);
    assert!(text.contains("duplicate envoy install"), "{text}");
    assert!(
        text.contains("--migrate") && text.contains("--repair") && text.contains("--uninstall"),
        "{text}"
    );
}

#[test]
fn system_envoy_required_is_reported_for_privileged_jobs_on_user_nodes() {
    let home = tempfile::tempdir().unwrap();
    let output = run_install(
        home.path(),
        &["--tier", "user", "--print-capabilities", "--dry-run"],
    );
    assert!(output.status.success(), "{}", combined(&output));
    let text = combined(&output);
    assert!(text.contains("agent_runtime"), "{text}");
    assert!(text.contains("rootless_workloads"), "{text}");
    assert!(text.contains("SYSTEM_ENVOY_REQUIRED"), "{text}");
}
