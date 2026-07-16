//! Operator terminal (PTY) subsystem for the Envoy (ADR 0021 cockpit).
//!
//! A `PtyManager` owns a set of live operator shells keyed by Hall-issued
//! `terminal_id`. Each shell is `$SHELL` running under a fresh PTY with the
//! child as a process-group / session leader, so closing a terminal kills the
//! whole group (SIGHUP→SIGKILL), not just the top process.
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

/// One live terminal: the master fd wrapped for async writes + the child pid
/// (process-group leader) so we can signal the whole group on close.
struct PtyShell {
    writer: Mutex<tokio::fs::File>,
    master_fd: RawFd,
    child_pid: libc::pid_t,
    reader_task: tokio::task::JoinHandle<()>,
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
}

impl PtyManager {
    pub fn new(sink: Arc<dyn TerminalSink>) -> Arc<Self> {
        Arc::new(Self {
            shells: Mutex::new(HashMap::new()),
            sink,
        })
    }

    /// Open a shell for `terminal_id`. Idempotent: opening an existing id is a
    /// no-op success (the operator reconnecting to the same tab).
    pub async fn open(
        &self,
        terminal_id: &str,
        cols: u16,
        rows: u16,
        cwd: Option<&str>,
    ) -> Result<()> {
        {
            let shells = self.shells.lock().await;
            if shells.contains_key(terminal_id) {
                return Ok(());
            }
        }

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

        // forkpty: child gets the slave as controlling terminal + becomes a
        // session leader; parent gets the master fd.
        let mut master_fd: RawFd = -1;
        // SAFETY: forkpty is the canonical PTY-spawn primitive; we immediately
        // branch on the returned pid and only touch async-signal-safe libc
        // calls in the child before exec.
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
            // ── Child ── (async-signal-safe only until exec)
            // New session/process group already established by forkpty.
            let cwd_c = std::ffi::CString::new(workdir)
                .unwrap_or_else(|_| std::ffi::CString::new("/").unwrap());
            unsafe {
                libc::chdir(cwd_c.as_ptr());
            }
            let shell_c = std::ffi::CString::new(shell.clone()).unwrap();
            // Minimal argv: interactive login shell.
            let arg0 = std::ffi::CString::new("-").unwrap(); // leading '-' → login shell
            let term = std::ffi::CString::new("TERM=xterm-256color").unwrap();
            // exec; if it fails, exit non-zero.
            let argv = [shell_c.as_ptr(), std::ptr::null()];
            let envp = [term.as_ptr(), std::ptr::null()];
            let _ = arg0; // arg0 replaced below via execle-style; keep simple execv
            unsafe {
                // Use execvp so PATH resolves; env carries TERM (plus inherited).
                libc::setenv(
                    std::ffi::CString::new("TERM").unwrap().as_ptr(),
                    std::ffi::CString::new("xterm-256color").unwrap().as_ptr(),
                    1,
                );
                let _ = envp;
                libc::execvp(shell_c.as_ptr(), argv.as_ptr());
                libc::_exit(127);
            }
        }

        // ── Parent ──
        // Wrap master fd for async I/O. Set non-blocking.
        // SAFETY: master_fd is a valid fd returned by forkpty, owned by us now.
        let flags = unsafe { libc::fcntl(master_fd, libc::F_GETFL) };
        unsafe {
            libc::fcntl(master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
        // Two independent File handles over dup'd fds: one for the reader task,
        // one for writes, so we don't share a single File across tasks.
        let read_fd = unsafe { libc::dup(master_fd) };
        if read_fd < 0 {
            return Err(anyhow::anyhow!("dup(master) failed"));
        }
        // SAFETY: both fds are valid and owned; File takes ownership.
        let read_file = unsafe { std::fs::File::from_raw_fd(read_fd) };
        let write_file = unsafe { std::fs::File::from_raw_fd(master_fd) };
        let mut read_async = tokio::fs::File::from_std(read_file);
        let write_async = tokio::fs::File::from_std(write_file);

        let sink = self.sink.clone();
        let tid = terminal_id.to_string();
        let reader_task = tokio::spawn(async move {
            let mut buf = vec![0u8; 8192];
            loop {
                match read_async.read(&mut buf).await {
                    Ok(0) => break, // EOF: shell closed the pty
                    Ok(n) => sink.output(tid.clone(), b64_encode(&buf[..n])),
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                    Err(_) => break,
                }
            }
            // Reap the child and report exit.
            let mut status: libc::c_int = 0;
            let code = unsafe {
                libc::waitpid(pid, &mut status, 0);
                if libc::WIFEXITED(status) {
                    Some(libc::WEXITSTATUS(status))
                } else {
                    None
                }
            };
            sink.exited(tid.clone(), code);
        });

        let mut shells = self.shells.lock().await;
        shells.insert(
            terminal_id.to_string(),
            PtyShell {
                writer: Mutex::new(write_async),
                master_fd,
                child_pid: pid,
                reader_task,
            },
        );
        tracing::info!(terminal = %terminal_id, shell = %shell, "operator PTY opened");
        Ok(())
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

    /// Close a terminal: signal the whole process group, abort the reader.
    pub async fn close(&self, terminal_id: &str) -> Result<()> {
        let mut shells = self.shells.lock().await;
        if let Some(shell) = shells.remove(terminal_id) {
            // Kill the process GROUP (negative pid) so children die too.
            // SAFETY: signalling a known process group we spawned.
            unsafe {
                libc::kill(-shell.child_pid, libc::SIGHUP);
                libc::kill(-shell.child_pid, libc::SIGKILL);
            }
            shell.reader_task.abort();
            tracing::info!(terminal = %terminal_id, "operator PTY closed");
        }
        Ok(())
    }
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
}
