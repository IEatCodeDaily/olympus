//! Opt-in shell job execution for the envoy (ADR 0011 phase 1).

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::future::join;
use olympus_proto::frames::{EnvoyFrame, JobStream};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::task::JoinHandle;

struct JobEntry {
    child: Arc<Mutex<Child>>,
    cancelled: Arc<AtomicBool>,
}

pub struct JobSpec {
    pub job_id: String,
    pub argv: Vec<String>,
    pub env_allowlist: Vec<String>,
    pub cwd: Option<String>,
    pub timeout_secs: u64,
    pub max_output_bytes: u64,
}

/// Active jobs. Jobs are unrelated to ACP runtimes and have their own table.
#[derive(Clone)]
pub struct JobTable {
    root: Arc<PathBuf>,
    jobs: Arc<RwLock<HashMap<String, JobEntry>>>,
}

impl JobTable {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)
            .with_context(|| format!("creating job workspace {}", root.display()))?;
        Ok(Self {
            root: Arc::new(root),
            jobs: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn spawn(&self, spec: JobSpec, tx: mpsc::UnboundedSender<EnvoyFrame>) -> Result<()> {
        let program = spec
            .argv
            .first()
            .filter(|v| !v.is_empty())
            .context("argv must contain a non-empty program")?
            .clone();
        if self.jobs.read().await.contains_key(&spec.job_id) {
            anyhow::bail!("job already exists: {}", spec.job_id);
        }
        let cwd = resolve_cwd(&self.root, spec.cwd.as_deref())?;
        std::fs::create_dir_all(&cwd)?;
        let mut command = Command::new(program);
        command
            .args(&spec.argv[1..])
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        command.env_clear();
        for name in &spec.env_allowlist {
            if let Ok(value) = std::env::var(name) {
                command.env(name, value);
            }
        }
        let mut child = command.spawn().context("spawning job")?;
        let stdout = child.stdout.take().context("capturing stdout")?;
        let stderr = child.stderr.take().context("capturing stderr")?;
        let child = Arc::new(Mutex::new(child));
        let cancelled = Arc::new(AtomicBool::new(false));
        self.jobs.write().await.insert(
            spec.job_id.clone(),
            JobEntry {
                child: child.clone(),
                cancelled: cancelled.clone(),
            },
        );

        let emitted = Arc::new(AtomicU64::new(0));
        let truncated = Arc::new(AtomicBool::new(false));
        let stdout_task = stream_output(
            stdout,
            spec.job_id.clone(),
            JobStream::Stdout,
            spec.max_output_bytes,
            emitted.clone(),
            truncated.clone(),
            tx.clone(),
        );
        let stderr_task = stream_output(
            stderr,
            spec.job_id.clone(),
            JobStream::Stderr,
            spec.max_output_bytes,
            emitted,
            truncated.clone(),
            tx.clone(),
        );

        let jobs = self.jobs.clone();
        tokio::spawn(async move {
            let deadline =
                tokio::time::Instant::now() + Duration::from_secs(spec.timeout_secs.max(1));
            let mut timed_out = false;
            let exit_code = loop {
                let status = { child.lock().await.try_wait() };
                match status {
                    Ok(Some(status)) => break status.code(),
                    Ok(None) if tokio::time::Instant::now() >= deadline => {
                        timed_out = true;
                        let _ = child.lock().await.kill().await;
                        break child.lock().await.wait().await.ok().and_then(|s| s.code());
                    }
                    Ok(None) => tokio::time::sleep(Duration::from_millis(25)).await,
                    Err(_) => break None,
                }
            };
            let _ = join(stdout_task, stderr_task).await;
            jobs.write().await.remove(&spec.job_id);
            let _ = tx.send(EnvoyFrame::JobResult {
                job_id: spec.job_id,
                seq: 0,
                exit_code,
                truncated: truncated.load(Ordering::Relaxed),
                timed_out,
                cancelled: cancelled.load(Ordering::Relaxed),
            });
        });
        Ok(())
    }

    pub async fn cancel(&self, job_id: &str) -> Result<()> {
        let jobs = self.jobs.read().await;
        let entry = jobs
            .get(job_id)
            .with_context(|| format!("unknown job: {job_id}"))?;
        entry.cancelled.store(true, Ordering::Relaxed);
        let child = entry.child.clone();
        drop(jobs);
        let result = child.lock().await.kill().await.context("killing job");
        result
    }
}

fn resolve_cwd(root: &Path, cwd: Option<&str>) -> Result<PathBuf> {
    let relative = Path::new(cwd.unwrap_or("."));
    if relative.is_absolute()
        || relative.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        anyhow::bail!("cwd must stay within the envoy job workspace root");
    }
    Ok(root.join(relative))
}

fn stream_output<R: tokio::io::AsyncRead + Send + Unpin + 'static>(
    mut reader: R,
    job_id: String,
    stream: JobStream,
    limit: u64,
    emitted: Arc<AtomicU64>,
    truncated: Arc<AtomicBool>,
    tx: mpsc::UnboundedSender<EnvoyFrame>,
) -> JoinHandle<()> {
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
            let _ = tx.send(EnvoyFrame::JobOutput {
                job_id: job_id.clone(),
                seq: 0,
                stream,
                data: String::from_utf8_lossy(&buffer[..keep]).into_owned(),
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runs_argv_and_streams_output() {
        let dir = tempfile::tempdir().unwrap();
        let table = JobTable::new(dir.path().into()).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        table
            .spawn(
                JobSpec {
                    job_id: "j".into(),
                    argv: vec!["printf".into(), "hello".into()],
                    env_allowlist: vec![],
                    cwd: None,
                    timeout_secs: 5,
                    max_output_bytes: 100,
                },
                tx,
            )
            .await
            .unwrap();
        let mut output = String::new();
        loop {
            match rx.recv().await.unwrap() {
                EnvoyFrame::JobOutput { data, .. } => output.push_str(&data),
                EnvoyFrame::JobResult { exit_code, .. } => {
                    assert_eq!(exit_code, Some(0));
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(output, "hello");
    }

    #[test]
    fn cwd_cannot_escape_root() {
        assert!(resolve_cwd(Path::new("/tmp/root"), Some("../nope")).is_err());
    }
}
