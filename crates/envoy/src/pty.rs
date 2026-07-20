//! Operator terminal (PTY) subsystem for the Envoy (ADR 0021 cockpit).
//!
//! A `PtyManager` owns a set of live operator shells keyed by Hall-issued
//! `terminal_id`. Each shell is `$SHELL` running inside a **tmux session**
//! (`olympus-term-<id>`). Tmux provides persistence: the shell survives a
//! WebSocket disconnect and can be re-attached on reconnect. If tmux is not
//! installed, the manager falls back to a bare `forkpty` shell and reports
//! `persistent=false` so the UI can show a "non-persistent" badge.
//!
//! This is **operator-only**. Nothing here is reachable by an agent runtime:
//! the manager is driven exclusively by `HallFrame::Terminal*` frames, which
//! Hall only emits from the operator-authenticated cockpit WebSocket.
//!
//! Bytes are opaque (not guaranteed UTF-8) and cross the Hall wire base64.

use std::collections::HashMap;
use std::os::fd::{FromRawFd, RawFd};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

/// A base64 alphabet encode/decode without pulling a crate — PTY payloads are
/// small and this keeps the dependency surface minimal.
mod b64 {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(input: &[u8]) -> String {
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
            out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[((n >> 6) & 63) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[(n & 63) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    pub fn decode(input: &str) -> Option<Vec<u8>> {
        fn val(c: u8) -> Option<u32> {
            match c {
                b'A'..=b'Z' => Some((c - b'A') as u32),
                b'a'..=b'z' => Some((c - b'a' + 26) as u32),
                b'0'..=b'9' => Some((c - b'0' + 52) as u32),
                b'+' => Some(62),
                b'/' => Some(63),
                _ => None,
            }
        }
        let bytes: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
        for chunk in bytes.chunks(4) {
            if chunk.len() < 2 {
                return None;
            }
            let pad = chunk.iter().filter(|&&c| c == b'=').count();
            let mut n = 0u32;
            for (i, &c) in chunk.iter().enumerate() {
                let v = if c == b'=' { 0 } else { val(c)? };
                n |= v << (18 - 6 * i);
            }
            out.push((n >> 16) as u8);
            if pad < 2 {
                out.push((n >> 8) as u8);
            }
            if pad < 1 {
                out.push(n as u8);
            }
        }
        Some(out)
    }
}

pub use b64::{decode as b64_decode, encode as b64_encode};

/// Session name used by tmux for a given terminal id.
pub fn tmux_session_name(terminal_id: &str) -> String {
    format!("olympus-term-{}", terminal_id)
}

/// Check whether tmux is available on PATH.
pub fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("list-clients")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// One live terminal attachment: the master fd wrapped for async writes, the
/// child pid (process-group leader for tmux client or bare shell), and the
/// reader task that drains output.
struct PtyShell {
    writer: Mutex<tokio::fs::File>,
    master_fd: RawFd,
    child_pid: libc::pid_t,
    reader_task: tokio::task::JoinHandle<()>,
    /// Whether this shell is backed by tmux (persistent) or bare forkpty.
    persistent: bool,
}

/// What the manager needs to emit output/exit frames back toward Hall.
pub trait TerminalSink: Send + Sync + 'static {
    /// Emit base64 output bytes for a terminal.
    fn output(&self, terminal_id: String, data_b64: String);
    /// Emit terminal-exited.
    fn exited(&self, terminal_id: String, exit_code: Option<i32>);
}

/// A `TerminalSink` that forwards to an mpsc channel of `(terminal_id,
/// Option<exit_code>, data)` — the connection layer drains this into
/// `EnvoyFrame::TerminalOutput` / `TerminalExited`. Keeps `pty.rs` free of the
/// proto frame types so it stays a pure PTY primitive.
pub enum TerminalMsg {
    Output {
        terminal_id: String,
        data_b64: String,
    },
    Exited {
        terminal_id: String,
        exit_code: Option<i32>,
    },
}

pub struct ChannelSink {
    tx: tokio::sync::mpsc::UnboundedSender<TerminalMsg>,
}

impl ChannelSink {
    pub fn new() -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<TerminalMsg>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Arc::new(Self { tx }), rx)
    }
}

impl TerminalSink for ChannelSink {
    fn output(&self, terminal_id: String, data_b64: String) {
        let _ = self.tx.send(TerminalMsg::Output {
            terminal_id,
            data_b64,
        });
    }
    fn exited(&self, terminal_id: String, exit_code: Option<i32>) {
        let _ = self.tx.send(TerminalMsg::Exited {
            terminal_id,
            exit_code,
        });
    }
}

pub struct PtyManager {
    shells: Mutex<HashMap<String, PtyShell>>,
    sink: Arc<dyn TerminalSink>,
    /// Override for tests: force tmux path (true) or bare-forkpty (false).
    /// In production this is `None` → auto-detect.
    use_tmux_override: std::sync::Mutex<Option<bool>>,
}

impl PtyManager {
    pub fn new(sink: Arc<dyn TerminalSink>) -> Arc<Self> {
        Arc::new(Self {
            shells: Mutex::new(HashMap::new()),
            sink,
            use_tmux_override: std::sync::Mutex::new(None),
        })
    }

    /// Whether this manager is using tmux for persistence (runtime query).
    pub fn persistent_enabled(&self) -> bool {
        if let Ok(g) = self.use_tmux_override.lock() {
            if let Some(v) = *g {
                return v;
            }
        }
        tmux_available()
    }

    /// Test-only override for tmux detection.
    #[cfg(test)]
    pub fn set_tmux_override(&self, enabled: Option<bool>) {
        *self.use_tmux_override.lock().unwrap() = enabled;
    }

    /// Open a shell for `terminal_id`. If tmux is available and a session
    /// `olympus-term-<id>` already exists, re-attach to it (reconnect). If the
    /// session does not exist, create it then attach. If tmux is absent, fall
    /// back to bare `forkpty`.
    pub async fn open(
        &self,
        terminal_id: &str,
        cols: u16,
        rows: u16,
        cwd: Option<&str>,
    ) -> Result<bool> {
        {
            let shells = self.shells.lock().await;
            if shells.contains_key(terminal_id) {
                return Ok(shells[terminal_id].persistent);
            }
        }

        let persistent = self.persistent_enabled();
        let session = tmux_session_name(terminal_id);

        let (master_fd, child_pid) = if persistent {
            spawn_tmux_attach(&session, terminal_id, cols, rows, cwd)?
        } else {
            spawn_bare_shell(cols, rows, cwd)?
        };

        // Set non-blocking.
        let flags = unsafe { libc::fcntl(master_fd, libc::F_GETFL) };
        unsafe {
            libc::fcntl(master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        let read_fd = unsafe { libc::dup(master_fd) };
        if read_fd < 0 {
            return Err(anyhow::anyhow!("dup(master) failed"));
        }
        let read_file = unsafe { std::fs::File::from_raw_fd(read_fd) };
        let write_file = unsafe { std::fs::File::from_raw_fd(master_fd) };
        let mut read_async = tokio::fs::File::from_std(read_file);
        let write_async = tokio::fs::File::from_std(write_file);

        let sink = self.sink.clone();
        let tid = terminal_id.to_string();
        let persistent_reader = persistent;
        let reader_task = tokio::spawn(async move {
            let mut buf = vec![0u8; 8192];
            loop {
                match read_async.read(&mut buf).await {
                    Ok(0) => break, // EOF: client/shell closed the pty
                    Ok(n) => sink.output(tid.clone(), b64_encode(&buf[..n])),
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                    Err(_) => break,
                }
            }
            // For bare (non-persistent) shells, reap the child and report exit.
            // For tmux-backed shells, EOF just means the *client* detached — the
            // tmux server keeps the shell alive. Only report exit if the tmux
            // session itself is gone.
            if persistent_reader {
                let still_alive = std::process::Command::new("tmux")
                    .args(["has-session", "-t", &tmux_session_name(&tid)])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if !still_alive {
                    let mut status: libc::c_int = 0;
                    unsafe { libc::waitpid(child_pid, &mut status, 0) };
                    let code = if libc::WIFEXITED(status) {
                        Some(libc::WEXITSTATUS(status))
                    } else {
                        None
                    };
                    sink.exited(tid.clone(), code);
                }
            } else {
                let mut status: libc::c_int = 0;
                let code = unsafe {
                    libc::waitpid(child_pid, &mut status, 0);
                    if libc::WIFEXITED(status) {
                        Some(libc::WEXITSTATUS(status))
                    } else {
                        None
                    }
                };
                sink.exited(tid.clone(), code);
            }
        });

        let mut shells = self.shells.lock().await;
        shells.insert(
            terminal_id.to_string(),
            PtyShell {
                writer: Mutex::new(write_async),
                master_fd,
                child_pid,
                reader_task,
                persistent,
            },
        );
        tracing::info!(
            terminal = %terminal_id,
            persistent,
            "operator PTY opened"
        );
        Ok(persistent)
    }

    /// Write operator keystrokes to a terminal.
    pub async fn input(&self, terminal_id: &str, data_b64: &str) -> Result<()> {
        let data = b64_decode(data_b64).context("bad base64 terminal input")?;
        let shells = self.shells.lock().await;
        let shell = shells
            .get(terminal_id)
            .ok_or_else(|| anyhow::anyhow!("no such terminal"))?;
        let mut w = shell.writer.lock().await;
        w.write_all(&data).await?;
        w.flush().await?;
        Ok(())
    }

    /// Resize a terminal's window.
    pub async fn resize(&self, terminal_id: &str, cols: u16, rows: u16) -> Result<()> {
        let shells = self.shells.lock().await;
        let shell = shells
            .get(terminal_id)
            .ok_or_else(|| anyhow::anyhow!("no such terminal"))?;
        let winsize = libc::winsize {
            ws_row: rows.max(1),
            ws_col: cols.max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: master_fd is a live fd owned by this shell.
        unsafe {
            libc::ioctl(shell.master_fd, libc::TIOCSWINSZ, &winsize);
        }
        Ok(())
    }

    /// Detach a terminal attachment: abort the reader task and drop the entry
    /// from the map, but leave the underlying tmux session alive so a later
    /// `open()` re-attaches. For non-persistent shells this is equivalent to
    /// close (the shell dies when the PTY closes).
    pub async fn detach(&self, terminal_id: &str) -> Result<()> {
        let mut shells = self.shells.lock().await;
        if let Some(shell) = shells.remove(terminal_id) {
            shell.reader_task.abort();
            // PtyShell drops here: writer File closes master_fd, no manual close
            // needed (double-close → IO safety violation).
            tracing::debug!(terminal = %terminal_id, "operator PTY detached");
        }
        Ok(())
    }

    /// Close a terminal permanently: kill the tmux session (or bare shell
    /// process group), abort the reader.
    pub async fn close(&self, terminal_id: &str) -> Result<()> {
        let mut shells = self.shells.lock().await;
        if let Some(shell) = shells.remove(terminal_id) {
            shell.reader_task.abort();
            if shell.persistent {
                // Kill the tmux session so the shell dies.
                let session = tmux_session_name(terminal_id);
                let _ = std::process::Command::new("tmux")
                    .args(["kill-session", "-t", &session])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            // Kill the process group (covers both tmux client and bare shell).
            // SAFETY: signalling a known process group we spawned.
            unsafe {
                libc::kill(-shell.child_pid, libc::SIGHUP);
                libc::kill(-shell.child_pid, libc::SIGKILL);
            }
            // PtyShell drops here: writer File closes master_fd.
            tracing::info!(terminal = %terminal_id, "operator PTY closed");
        }
        Ok(())
    }
}

/// Spawn or re-attach to a tmux session and return the (master_fd, client_pid).
///
/// If the session already exists → `tmux attach`. If not → `tmux new-session`
/// then attach. We use `forkpty` to create a PTY pair, then `execvp("tmux")`
/// in the child so tmux's client runs as the process-group leader.
fn spawn_tmux_attach(
    session: &str,
    _terminal_id: &str,
    cols: u16,
    rows: u16,
    cwd: Option<&str>,
) -> Result<(RawFd, libc::pid_t)> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let workdir = cwd
        .filter(|c| !c.is_empty())
        .map(|c| c.to_string())
        .unwrap_or_else(|| home.clone());

    let winsize = libc::winsize {
        ws_row: rows.max(1),
        ws_col: cols.max(1),
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    // Check if the session already exists.
    let session_exists = std::process::Command::new("tmux")
        .args(["has-session", "-t", session])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let mut master_fd: RawFd = -1;
    // SAFETY: forkpty is the canonical PTY-spawn primitive.
    let pid = unsafe {
        libc::forkpty(
            &mut master_fd,
            std::ptr::null_mut(),
            std::ptr::null(),
            &winsize,
        )
    };
    if pid < 0 {
        return Err(anyhow::anyhow!(
            "forkpty failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if pid == 0 {
        // ── Child ── exec tmux attach or new-session.
        // Build args depending on whether the session exists.
        let cwd_c = std::ffi::CString::new(workdir)
            .unwrap_or_else(|_| std::ffi::CString::new("/").unwrap());
        unsafe {
            libc::chdir(cwd_c.as_ptr());
            libc::setenv(
                std::ffi::CString::new("TERM").unwrap().as_ptr(),
                std::ffi::CString::new("xterm-256color").unwrap().as_ptr(),
                1,
            );
        }

        let tmux_c = std::ffi::CString::new("tmux").unwrap();
        let session_c = std::ffi::CString::new(session).unwrap();

        if session_exists {
            // Attach to existing session.
            let attach_c = std::ffi::CString::new("attach-session").unwrap();
            let target_c = std::ffi::CString::new("-t").unwrap();
            let argv = [
                tmux_c.as_ptr(),
                attach_c.as_ptr(),
                target_c.as_ptr(),
                session_c.as_ptr(),
                std::ptr::null(),
            ];
            unsafe {
                libc::execvp(tmux_c.as_ptr(), argv.as_ptr());
                libc::_exit(127);
            }
        } else {
            // Create a new session with the user's shell.
            let new_c = std::ffi::CString::new("new-session").unwrap();
            let session_flag_c = std::ffi::CString::new("-s").unwrap();
            let shell_env_c = std::ffi::CString::new(shell).unwrap();
            let argv = [
                tmux_c.as_ptr(),
                new_c.as_ptr(),
                session_flag_c.as_ptr(),
                session_c.as_ptr(),
                shell_env_c.as_ptr(),
                std::ptr::null(),
            ];
            unsafe {
                libc::execvp(tmux_c.as_ptr(), argv.as_ptr());
                libc::_exit(127);
            }
        }
    }

    // ── Parent ──
    Ok((master_fd, pid))
}

/// Spawn a bare shell via forkpty (non-persistent fallback when tmux is absent).
fn spawn_bare_shell(cols: u16, rows: u16, cwd: Option<&str>) -> Result<(RawFd, libc::pid_t)> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let workdir = cwd
        .filter(|c| !c.is_empty())
        .map(|c| c.to_string())
        .unwrap_or_else(|| home.clone());

    let winsize = libc::winsize {
        ws_row: rows.max(1),
        ws_col: cols.max(1),
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    let mut master_fd: RawFd = -1;
    // SAFETY: forkpty is the canonical PTY-spawn primitive.
    let pid = unsafe {
        libc::forkpty(
            &mut master_fd,
            std::ptr::null_mut(),
            std::ptr::null(),
            &winsize,
        )
    };
    if pid < 0 {
        return Err(anyhow::anyhow!(
            "forkpty failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if pid == 0 {
        // ── Child ──
        let cwd_c = std::ffi::CString::new(workdir)
            .unwrap_or_else(|_| std::ffi::CString::new("/").unwrap());
        unsafe {
            libc::chdir(cwd_c.as_ptr());
        }
        let shell_c = std::ffi::CString::new(shell.clone()).unwrap();
        let argv = [shell_c.as_ptr(), std::ptr::null()];
        unsafe {
            libc::setenv(
                std::ffi::CString::new("TERM").unwrap().as_ptr(),
                std::ffi::CString::new("xterm-256color").unwrap().as_ptr(),
                1,
            );
            libc::execvp(shell_c.as_ptr(), argv.as_ptr());
            libc::_exit(127);
        }
    }

    // ── Parent ──
    Ok((master_fd, pid))
}

#[cfg(test)]
mod tests {
    use super::b64::{decode, encode};

    #[test]
    fn base64_round_trips_arbitrary_bytes() {
        for case in [
            &b""[..],
            b"a",
            b"ab",
            b"abc",
            b"abcd",
            &[0u8, 255, 128, 1, 2, 3][..],
            b"hello world\n\x1b[0m",
        ] {
            let enc = encode(case);
            let dec = decode(&enc).expect("decode");
            assert_eq!(dec, case, "round-trip for {enc}");
        }
    }

    #[test]
    fn base64_matches_known_vector() {
        assert_eq!(encode(b"Man"), "TWFu");
        assert_eq!(encode(b"Ma"), "TWE=");
        assert_eq!(encode(b"M"), "TQ==");
        assert_eq!(decode("TWFu").unwrap(), b"Man");
    }

    #[test]
    fn tmux_session_name_is_stable() {
        assert_eq!(tmux_session_name("abc-123"), "olympus-term-abc-123");
        assert_eq!(tmux_session_name("x"), "olympus-term-x");
    }

    /// A counting sink that records all messages in order, for lifecycle tests.
    struct RecordingSink {
        msgs: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl RecordingSink {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                msgs: std::sync::Mutex::new(Vec::new()),
            })
        }
    }

    impl TerminalSink for RecordingSink {
        fn output(&self, terminal_id: String, data_b64: String) {
            self.msgs
                .lock()
                .unwrap()
                .push((terminal_id, format!("output:{data_b64}")));
        }
        fn exited(&self, terminal_id: String, exit_code: Option<i32>) {
            self.msgs
                .lock()
                .unwrap()
                .push((terminal_id, format!("exited:{exit_code:?}")));
        }
    }

    use super::*;

    /// End-to-end lifecycle test: open → detach → re-open → close.
    /// Requires tmux installed. Skips gracefully if not.
    #[tokio::test]
    async fn tmux_lifecycle_open_detach_reattach_close() {
        if !tmux_available() {
            eprintln!("skipping tmux lifecycle test — tmux not installed");
            return;
        }

        let sink = RecordingSink::new();
        let mgr = PtyManager::new(sink.clone() as Arc<dyn TerminalSink>);
        let tid = "test-lifecycle";

        // Clean up any stale session.
        let session = tmux_session_name(tid);
        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", &session])
            .status();

        // Open: creates session + attaches.
        let persistent = mgr.open(tid, 80, 24, None).await.expect("open");
        assert!(persistent, "should be tmux-backed");

        // Give the shell a moment to start.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // Detach: the PTY client goes away but the session stays.
        mgr.detach(tid).await.expect("detach");

        // Verify session still exists.
        let session_alive = std::process::Command::new("tmux")
            .args(["has-session", "-t", &session])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(session_alive, "tmux session must survive detach");

        // Re-open: should re-attach to the same session.
        let persistent2 = mgr.open(tid, 80, 24, None).await.expect("reopen");
        assert!(persistent2, "re-open should also be tmux-backed");

        // Close: kills the session.
        mgr.close(tid).await.expect("close");

        // Verify session is gone.
        let session_dead = !std::process::Command::new("tmux")
            .args(["has-session", "-t", &session])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(session_dead, "tmux session must be killed on close");
    }

    /// Verify that open() returns false (non-persistent) when tmux is forced off.
    #[tokio::test]
    async fn bare_shell_when_tmux_disabled() {
        let sink = RecordingSink::new();
        let mgr = PtyManager::new(sink.clone() as Arc<dyn TerminalSink>);
        mgr.set_tmux_override(Some(false));

        let tid = "test-bare";
        let persistent = mgr.open(tid, 80, 24, None).await.expect("open");
        assert!(
            !persistent,
            "should be bare (non-persistent) when tmux disabled"
        );

        // Clean up.
        mgr.close(tid).await.ok();
    }
}
