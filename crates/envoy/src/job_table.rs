//! Durable, fenced shell job execution for the Envoy (ADR 0017 §5).

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use olympus_proto::frames::{EnvoyFrame, JobAttemptState, JobAttemptStatus, JobStream};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot, RwLock};

struct JobEntry {
    pid: u32,
    cancelled: Arc<AtomicBool>,
}

const MAX_RETAINED_TERMINAL_ATTEMPTS: usize = 1024;

#[derive(Clone, Serialize, Deserialize)]
struct RetainedAttempt {
    #[serde(flatten)]
    status: JobAttemptStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pgid: Option<u32>,
    #[serde(default)]
    updated_at: u64,
}

pub struct JobSpec {
    pub job_id: String,
    pub attempt_epoch: u64,
    pub package_id: String,
    pub package_version: String,
    pub package_digest: String,
    pub activity: String,
    pub argv: Vec<String>,
    pub env_allowlist: Vec<String>,
    pub cwd: Option<String>,
    pub timeout_secs: u64,
    pub max_output_bytes: u64,
}

pub struct JobFrame {
    pub frame: EnvoyFrame,
    pub persisted: Option<oneshot::Sender<Result<u64, String>>>,
}

#[derive(Clone)]
pub struct JobTable {
    root: Arc<PathBuf>,
    jobs: Arc<RwLock<HashMap<String, JobEntry>>>,
    attempts: Arc<RwLock<HashMap<String, RetainedAttempt>>>,
    attempts_path: Arc<PathBuf>,
}

impl JobTable {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)
            .with_context(|| format!("creating job workspace {}", root.display()))?;
        let attempts_path = root.join("attempts.json");
        let mut attempts: HashMap<String, RetainedAttempt> = std::fs::read(&attempts_path)
            .ok()
            .map(|bytes| serde_json::from_slice(&bytes))
            .transpose()?
            .unwrap_or_default();
        for attempt in attempts.values_mut() {
            if attempt.status.state == JobAttemptState::Running {
                if let Some(pgid) = attempt.pgid {
                    kill_group(pgid);
                }
                attempt.status.state = JobAttemptState::StepIndeterminate;
                attempt.status.terminal_reason =
                    Some("envoy_restarted_while_attempt_running".into());
                attempt.pgid = None;
                attempt.updated_at = now_millis();
            }
        }
        prune_attempts(&mut attempts);
        persist_attempts(&attempts_path, &attempts)?;
        Ok(Self {
            root: Arc::new(root),
            jobs: Arc::new(RwLock::new(HashMap::new())),
            attempts: Arc::new(RwLock::new(attempts)),
            attempts_path: Arc::new(attempts_path),
        })
    }

    pub async fn inventory(&self) -> Vec<JobAttemptStatus> {
        self.attempts
            .read()
            .await
            .values()
            .map(|attempt| attempt.status.clone())
            .collect()
    }

    pub async fn spawn(&self, spec: JobSpec, tx: mpsc::Sender<JobFrame>) -> Result<()> {
        let key = key(&spec.job_id, spec.attempt_epoch);
        if self.attempts.read().await.contains_key(&key) {
            return Ok(()); // idempotent duplicate dispatch attaches to retained attempt
        }
        let program = spec
            .argv
            .first()
            .filter(|value| !value.is_empty())
            .context("argv must contain a non-empty program")?
            .clone();
        // JOBS-2 is transport-only. EXEC-1 binds the package identity fields
        // to an installed immutable package and declared grant.
        let cwd = resolve_cwd(&self.root, spec.cwd.as_deref())?;
        std::fs::create_dir_all(&cwd)?;

        self.update_attempt(
            JobAttemptStatus {
                job_id: spec.job_id.clone(),
                attempt_epoch: spec.attempt_epoch,
                state: JobAttemptState::Running,
                exit_code: None,
                truncated: false,
                timed_out: false,
                cancelled: false,
                terminal_reason: None,
                final_sequence: None,
            },
            None,
        )
        .await?;

        let mut command = gated_command(program, &spec.argv[1..], &cwd, &spec.env_allowlist);
        let mut child = match command.spawn().context("spawning job") {
            Ok(child) => child,
            Err(error) => {
                self.mark_indeterminate(&spec.job_id, spec.attempt_epoch, "spawn_failed")
                    .await?;
                return Err(error);
            }
        };
        let pid = child.id().context("spawned job has no pid")?;
        self.update_attempt(
            JobAttemptStatus {
                job_id: spec.job_id.clone(),
                attempt_epoch: spec.attempt_epoch,
                state: JobAttemptState::Running,
                exit_code: None,
                truncated: false,
                timed_out: false,
                cancelled: false,
                terminal_reason: None,
                final_sequence: None,
            },
            Some(pid),
        )
        .await?;
        use tokio::io::AsyncWriteExt;
        child
            .stdin
            .take()
            .context("capturing job launch gate")?
            .write_all(b"go\n")
            .await?;
        let stdout = child.stdout.take().context("capturing stdout")?;
        let stderr = child.stderr.take().context("capturing stderr")?;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.jobs.write().await.insert(
            key.clone(),
            JobEntry {
                pid,
                cancelled: cancelled.clone(),
            },
        );

        let emitted = Arc::new(AtomicU64::new(0));
        let truncated = Arc::new(AtomicBool::new(false));
        let stdout = stream_output(
            stdout,
            spec.job_id.clone(),
            spec.attempt_epoch,
            JobStream::Stdout,
            spec.max_output_bytes,
            emitted.clone(),
            truncated.clone(),
            tx.clone(),
        );
        let stderr = stream_output(
            stderr,
            spec.job_id.clone(),
            spec.attempt_epoch,
            JobStream::Stderr,
            spec.max_output_bytes,
            emitted,
            truncated.clone(),
            tx.clone(),
        );

        let table = self.clone();
        tokio::spawn(async move {
            let deadline =
                tokio::time::Instant::now() + Duration::from_secs(spec.timeout_secs.max(1));
            let mut timed_out = false;
            let exit_code = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status.code(),
                    Ok(None) if tokio::time::Instant::now() >= deadline => {
                        timed_out = true;
                        kill_group(pid);
                        break child.wait().await.ok().and_then(|status| status.code());
                    }
                    Ok(None) => tokio::time::sleep(Duration::from_millis(25)).await,
                    Err(_) => break None,
                }
            };
            // Descendants can retain pipes after the leader exits. Keep the
            // deadline active until both drains join, then kill the whole tree.
            let mut stdout = stdout;
            let mut stderr = stderr;
            if tokio::time::timeout_at(deadline, async {
                let _ = tokio::join!(&mut stdout, &mut stderr);
            })
            .await
            .is_err()
            {
                timed_out = true;
                kill_group(pid);
                let _ = tokio::join!(stdout, stderr);
            }
            table.jobs.write().await.remove(&key);
            let cancelled = cancelled.load(Ordering::Relaxed);
            let reason = if timed_out {
                "timed_out"
            } else if cancelled {
                "cancelled"
            } else if exit_code == Some(0) {
                "succeeded"
            } else {
                "failed"
            };
            let (persisted_tx, persisted_rx) = oneshot::channel();
            let result = JobFrame {
                frame: EnvoyFrame::JobResult {
                    job_id: spec.job_id.clone(),
                    attempt_epoch: spec.attempt_epoch,
                    seq: 0,
                    exit_code,
                    truncated: truncated.load(Ordering::Relaxed),
                    timed_out,
                    cancelled,
                },
                persisted: Some(persisted_tx),
            };
            let final_sequence = match tx.send(result).await {
                Ok(()) => persisted_rx.await.ok().and_then(Result::ok),
                Err(_) => None,
            };
            if let Some(final_sequence) = final_sequence {
                let _ = table
                    .complete_attempt(JobAttemptStatus {
                        job_id: spec.job_id.clone(),
                        attempt_epoch: spec.attempt_epoch,
                        state: JobAttemptState::Completed,
                        exit_code,
                        truncated: truncated.load(Ordering::Relaxed),
                        timed_out,
                        cancelled,
                        terminal_reason: Some(reason.into()),
                        final_sequence: Some(final_sequence),
                    })
                    .await;
            } else {
                let _ = table
                    .mark_indeterminate(&spec.job_id, spec.attempt_epoch, "terminal_spool_loss")
                    .await;
            }
        });
        Ok(())
    }

    pub async fn cancel(&self, job_id: &str, attempt_epoch: u64) -> Result<()> {
        let jobs = self.jobs.read().await;
        let entry = jobs
            .get(&key(job_id, attempt_epoch))
            .with_context(|| format!("unknown job attempt: {job_id}/{attempt_epoch}"))?;
        entry.cancelled.store(true, Ordering::Relaxed);
        kill_group(entry.pid);
        Ok(())
    }

    pub async fn mark_indeterminate(
        &self,
        job_id: &str,
        attempt_epoch: u64,
        reason: &str,
    ) -> Result<()> {
        let pgid = self
            .jobs
            .read()
            .await
            .get(&key(job_id, attempt_epoch))
            .map(|entry| entry.pid);
        if let Some(pgid) = pgid {
            kill_group(pgid);
        }
        self.update_attempt(
            JobAttemptStatus {
                job_id: job_id.into(),
                attempt_epoch,
                state: JobAttemptState::StepIndeterminate,
                exit_code: None,
                truncated: false,
                timed_out: false,
                cancelled: false,
                terminal_reason: Some(reason.into()),
                final_sequence: None,
            },
            pgid,
        )
        .await
    }

    async fn update_attempt(&self, attempt: JobAttemptStatus, pgid: Option<u32>) -> Result<()> {
        let mut attempts = self.attempts.write().await;
        attempts.insert(
            key(&attempt.job_id, attempt.attempt_epoch),
            RetainedAttempt {
                status: attempt,
                pgid,
                updated_at: now_millis(),
            },
        );
        prune_attempts(&mut attempts);
        persist_attempts(&self.attempts_path, &attempts)
    }

    async fn complete_attempt(&self, attempt: JobAttemptStatus) -> Result<bool> {
        let mut attempts = self.attempts.write().await;
        let attempt_key = key(&attempt.job_id, attempt.attempt_epoch);
        if attempts
            .get(&attempt_key)
            .is_some_and(|current| current.status.state == JobAttemptState::StepIndeterminate)
        {
            return Ok(false);
        }
        attempts.insert(
            attempt_key,
            RetainedAttempt {
                status: attempt,
                pgid: None,
                updated_at: now_millis(),
            },
        );
        prune_attempts(&mut attempts);
        persist_attempts(&self.attempts_path, &attempts)?;
        Ok(true)
    }
}

fn gated_command(
    program: String,
    args: &[String],
    cwd: &Path,
    env_allowlist: &[String],
) -> Command {
    // The child waits for one byte before exec. If Envoy dies before the PGID
    // is durable, pipe EOF exits without running the requested program.
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "IFS= read -r _ || exit 125; exec \"$@\"", "job-gate"])
        .arg(program)
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    command.env_clear();
    for name in env_allowlist {
        if let Ok(value) = std::env::var(name) {
            command.env(name, value);
        }
    }
    // SAFETY: setsid is async-signal-safe and touches no Rust-managed state.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
}

fn kill_group(pid: u32) {
    // Negative pid targets the process group created by setsid.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

pub fn wire_id(job_id: &str, attempt_epoch: u64) -> String {
    format!("job:{job_id}:{attempt_epoch}")
}

fn key(job_id: &str, attempt_epoch: u64) -> String {
    format!("{job_id}:{attempt_epoch}")
}

fn persist_attempts(path: &Path, attempts: &HashMap<String, RetainedAttempt>) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        serde_json::to_writer(&mut file, attempts)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    std::fs::File::open(path.parent().context("attempt ledger has no parent")?)?.sync_all()?;
    Ok(())
}

fn prune_attempts(attempts: &mut HashMap<String, RetainedAttempt>) {
    let mut terminal = attempts
        .iter()
        .filter(|(_, attempt)| attempt.status.state != JobAttemptState::Running)
        .map(|(key, attempt)| (attempt.updated_at, key.clone()))
        .collect::<Vec<_>>();
    terminal.sort();
    let excess = terminal
        .len()
        .saturating_sub(MAX_RETAINED_TERMINAL_ATTEMPTS);
    for (_, key) in terminal.into_iter().take(excess) {
        attempts.remove(&key);
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

fn resolve_cwd(root: &Path, cwd: Option<&str>) -> Result<PathBuf> {
    let relative = Path::new(cwd.unwrap_or("."));
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        anyhow::bail!("cwd must stay within the envoy job workspace root");
    }
    Ok(root.join(relative))
}

#[allow(clippy::too_many_arguments)]
fn stream_output<R: tokio::io::AsyncRead + Send + Unpin + 'static>(
    mut reader: R,
    job_id: String,
    attempt_epoch: u64,
    stream: JobStream,
    limit: u64,
    emitted: Arc<AtomicU64>,
    truncated: Arc<AtomicBool>,
    tx: mpsc::Sender<JobFrame>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut buffer = vec![0_u8; 8192];
        while let Ok(count) = reader.read(&mut buffer).await {
            if count == 0 {
                break;
            }
            let before = emitted.fetch_add(count as u64, Ordering::Relaxed);
            if before >= limit {
                truncated.store(true, Ordering::Relaxed);
                continue;
            }
            let keep = count.min((limit - before) as usize);
            if keep < count {
                truncated.store(true, Ordering::Relaxed);
            }
            if tx
                .send(JobFrame {
                    frame: EnvoyFrame::JobOutput {
                        job_id: job_id.clone(),
                        attempt_epoch,
                        seq: 0,
                        stream,
                        data: String::from_utf8_lossy(&buffer[..keep]).into_owned(),
                    },
                    persisted: None,
                })
                .await
                .is_err()
            {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str, argv: Vec<String>) -> JobSpec {
        JobSpec {
            job_id: id.into(),
            attempt_epoch: 7,
            package_id: "core.jobs".into(),
            package_version: "0.1".into(),
            package_digest: "builtin:jobs-v1".into(),
            activity: "job.run".into(),
            argv,
            env_allowlist: vec![],
            cwd: None,
            timeout_secs: 5,
            max_output_bytes: 100,
        }
    }

    #[tokio::test]
    async fn launch_gate_prevents_effects_before_pgid_is_durable() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let mut command = gated_command(
            "/bin/sh".into(),
            &["-c".into(), format!("touch {}", marker.display())],
            dir.path(),
            &[],
        );
        let mut child = command.spawn().unwrap();
        drop(child.stdin.take()); // simulate Envoy crash before ledger fsync
        assert_eq!(child.wait().await.unwrap().code(), Some(125));
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn duplicate_dispatch_is_idempotent_and_drains_before_result() {
        let dir = tempfile::tempdir().unwrap();
        let table = JobTable::new(dir.path().into()).unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        table
            .spawn(spec("j", vec!["printf".into(), "hello".into()]), tx.clone())
            .await
            .unwrap();
        table
            .spawn(spec("j", vec!["false".into()]), tx)
            .await
            .unwrap();
        let mut output = String::new();
        loop {
            let pending = rx.recv().await.unwrap();
            match pending.frame {
                EnvoyFrame::JobOutput { data, .. } => output.push_str(&data),
                EnvoyFrame::JobResult { exit_code, .. } => {
                    pending.persisted.unwrap().send(Ok(1)).unwrap();
                    assert_eq!(exit_code, Some(0));
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(output, "hello");
    }

    #[tokio::test]
    async fn cancellation_kills_process_group_tree() {
        let dir = tempfile::tempdir().unwrap();
        let table = JobTable::new(dir.path().into()).unwrap();
        let pid_file = dir.path().join("child.pid");
        let script = format!("sleep 60 & echo $! > {}; wait", pid_file.display());
        let (tx, _rx) = mpsc::channel(16);
        table
            .spawn(spec("tree", vec!["sh".into(), "-c".into(), script]), tx)
            .await
            .unwrap();
        let child_pid: i32 = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(contents) = std::fs::read_to_string(&pid_file) {
                    if let Ok(pid) = contents.trim().parse() {
                        break pid;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("child pid was not written");
        table.cancel("tree", 7).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(unsafe { libc::kill(child_pid, 0) }, -1);
    }

    #[tokio::test]
    async fn timeout_survives_leader_exit_and_kills_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let table = JobTable::new(dir.path().into()).unwrap();
        let pid_file = dir.path().join("timeout-child.pid");
        let mut job = spec(
            "timeout-tree",
            vec![
                "sh".into(),
                "-c".into(),
                format!("sleep 60 & echo $! > {}", pid_file.display()),
            ],
        );
        job.timeout_secs = 1;
        let (tx, mut rx) = mpsc::channel(16);
        table.spawn(job, tx).await.unwrap();

        let result = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(pending) = rx.recv().await {
                    if let EnvoyFrame::JobResult { timed_out, .. } = pending.frame {
                        pending.persisted.unwrap().send(Ok(0)).unwrap();
                        break timed_out;
                    }
                }
            }
        })
        .await
        .expect("timed-out job never produced a terminal result");
        assert!(result);
        let child_pid: i32 = std::fs::read_to_string(pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(unsafe { libc::kill(child_pid, 0) }, -1);
    }

    #[test]
    fn restart_marks_running_attempt_indeterminate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("attempts.json");
        let mut attempts = HashMap::new();
        attempts.insert(
            "j:1".into(),
            RetainedAttempt {
                status: JobAttemptStatus {
                    job_id: "j".into(),
                    attempt_epoch: 1,
                    state: JobAttemptState::Running,
                    exit_code: None,
                    truncated: false,
                    timed_out: false,
                    cancelled: false,
                    terminal_reason: None,
                    final_sequence: None,
                },
                pgid: None,
                updated_at: 1,
            },
        );
        persist_attempts(&path, &attempts).unwrap();
        let table = JobTable::new(dir.path().into()).unwrap();
        let attempts = table.attempts.blocking_read();
        assert_eq!(
            attempts["j:1"].status.state,
            JobAttemptState::StepIndeterminate
        );
    }

    #[tokio::test]
    async fn output_loss_is_sticky_when_child_exits() {
        let dir = tempfile::tempdir().unwrap();
        let table = JobTable::new(dir.path().into()).unwrap();
        let (tx, _rx) = mpsc::channel(16);
        table
            .spawn(
                spec("loss", vec!["sh".into(), "-c".into(), "sleep 0.1".into()]),
                tx,
            )
            .await
            .unwrap();
        table
            .mark_indeterminate("loss", 7, "output_spool_loss")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;

        let attempt = table
            .inventory()
            .await
            .into_iter()
            .find(|attempt| attempt.job_id == "loss")
            .unwrap();
        assert_eq!(attempt.state, JobAttemptState::StepIndeterminate);
        assert_eq!(
            attempt.terminal_reason.as_deref(),
            Some("output_spool_loss")
        );
    }

    #[test]
    fn restart_kills_persisted_running_process_group() {
        use std::os::unix::process::CommandExt;

        let dir = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "sleep 60"]);
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        let pgid = child.id();
        std::fs::write(
            dir.path().join("attempts.json"),
            format!(
                "{{\"orphan:1\":{{\"jobId\":\"orphan\",\"attemptEpoch\":1,\"state\":\"running\",\"pgid\":{pgid}}}}}\n"
            ),
        )
        .unwrap();

        let _table = JobTable::new(dir.path().into()).unwrap();
        let exited = (0..20).any(|_| {
            if child.try_wait().unwrap().is_some() {
                true
            } else {
                std::thread::sleep(Duration::from_millis(10));
                false
            }
        });
        if !exited {
            kill_group(pgid);
            let _ = child.wait();
        }
        assert!(exited, "persisted process group survived envoy restart");
    }

    #[test]
    fn terminal_attempt_inventory_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let attempts = (0..=1024)
            .map(|index| {
                let id = format!("job-{index}");
                (
                    format!("{id}:1"),
                    RetainedAttempt {
                        status: JobAttemptStatus {
                            job_id: id,
                            attempt_epoch: 1,
                            state: JobAttemptState::Completed,
                            exit_code: Some(0),
                            truncated: false,
                            timed_out: false,
                            cancelled: false,
                            terminal_reason: Some("succeeded".into()),
                            final_sequence: Some(0),
                        },
                        pgid: None,
                        updated_at: index,
                    },
                )
            })
            .collect();
        persist_attempts(&dir.path().join("attempts.json"), &attempts).unwrap();

        let table = JobTable::new(dir.path().into()).unwrap();
        assert!(table.attempts.blocking_read().len() <= 1024);
    }
}
