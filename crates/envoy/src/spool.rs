//! Durable per-session outbound event spool (ADR 0008 §2).
//!
//! Records are compact JSON lines. An event is fsynced before it is eligible
//! for transport; acknowledgements atomically rewrite the file to retain only
//! records above Hall's durable watermark.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use olympus_proto::frames::EnvoyFrame;

pub const DEFAULT_SESSION_CAP: u64 = 512 * 1024 * 1024;

/// A process-wide spool. Sequence allocation lives here rather than in a
/// connection, so reconnects cannot reset a session's ordering key.
pub struct EventSpool {
    dir: PathBuf,
    cap: u64,
    next_seq: Mutex<HashMap<String, u64>>,
}

impl EventSpool {
    pub fn open(state_dir: &Path) -> Result<Self> {
        Self::with_cap(state_dir, DEFAULT_SESSION_CAP)
    }

    pub fn with_cap(state_dir: &Path, cap: u64) -> Result<Self> {
        let dir = state_dir.join("spool");
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let spool = Self {
            dir,
            cap,
            next_seq: Mutex::new(HashMap::new()),
        };
        spool.recover_all()?;
        Ok(spool)
    }

    pub fn next_seq(&self, session_id: &str) -> Result<u64> {
        let mut next = self.next_seq.lock().expect("spool sequence mutex poisoned");
        let value = match next.get(session_id).copied() {
            Some(value) => value,
            None => {
                let from_spool = self
                    .read(session_id, None)?
                    .last()
                    .and_then(event_seq)
                    .map_or(0, |seq| seq.saturating_add(1));
                let from_counter = fs::read_to_string(self.counter_path(session_id))
                    .ok()
                    .and_then(|raw| raw.trim().parse::<u64>().ok())
                    .unwrap_or(0);
                from_spool.max(from_counter)
            }
        };
        let following = value.checked_add(1).context("event sequence exhausted")?;
        next.insert(session_id.to_owned(), following);
        Ok(value)
    }

    /// Append and fsync an event before the caller sends it.
    pub fn append(&self, frame: &EnvoyFrame) -> Result<()> {
        let (session_id, seq) = event_identity(frame).context("only event frames are spoolable")?;
        let bytes = serde_json::to_vec(frame).context("serializing spooled event")?;
        let path = self.path(session_id);
        let current = fs::metadata(&path).map_or(0, |meta| meta.len());
        let projected = current
            .checked_add(bytes.len() as u64 + 1)
            .context("spool size overflow")?;
        if projected > self.cap {
            anyhow::bail!(
                "SPOOL_OVERFLOW: session {session_id} exceeded {} bytes",
                self.cap
            );
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        self.persist_counter(
            session_id,
            seq.checked_add(1).context("event sequence exhausted")?,
        )?;
        Ok(())
    }

    /// Return ordered records, optionally restricted to seq > watermark.
    pub fn read(&self, session_id: &str, after: Option<u64>) -> Result<Vec<EnvoyFrame>> {
        let path = self.path(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&path).with_context(|| format!("opening {}", path.display()))?;
        let mut frames = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let frame: EnvoyFrame = serde_json::from_str(&line)
                .with_context(|| format!("decoding recovered spool {}", path.display()))?;
            let (_, seq) = event_identity(&frame).context("non-event record in event spool")?;
            if after.is_none_or(|watermark| seq > watermark) {
                frames.push(frame);
            }
        }
        frames.sort_by_key(|frame| event_seq(frame).unwrap_or(u64::MAX));
        Ok(frames)
    }

    /// Atomically retain only records above Hall's acknowledged watermark.
    pub fn acknowledge(&self, session_id: &str, watermark: u64) -> Result<()> {
        let retained = self.read(session_id, Some(watermark))?;
        let path = self.path(session_id);
        if retained.is_empty() {
            match fs::remove_file(&path) {
                Ok(()) => sync_parent(&self.dir)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            return Ok(());
        }
        let tmp = path.with_extension("jsonl.tmp");
        {
            let mut file = File::create(&tmp)?;
            for frame in retained {
                serde_json::to_writer(&mut file, &frame)?;
                file.write_all(b"\n")?;
            }
            file.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        sync_parent(&self.dir)
    }

    pub fn sessions(&self) -> Result<Vec<String>> {
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str().and_then(|s| s.strip_suffix(".jsonl")) else {
                continue;
            };
            sessions.push(decode_name(name)?);
        }
        sessions.sort();
        Ok(sessions)
    }

    pub fn last_seq(&self, session_id: &str) -> Result<Option<u64>> {
        Ok(self.read(session_id, None)?.last().and_then(event_seq))
    }

    fn recover_all(&self) -> Result<()> {
        for session in self.sessions()? {
            self.recover_file(&self.path(&session))?;
        }
        Ok(())
    }

    /// Keep the longest valid, newline-terminated prefix. A crash can leave
    /// only the final append truncated; corruption in the middle discards the
    /// corrupt record and everything after it rather than inventing ordering.
    fn recover_file(&self, path: &Path) -> Result<()> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut reader = BufReader::new(file.try_clone()?);
        let mut valid_len = 0_u64;
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            if !line.ends_with(b"\n") {
                break;
            }
            let record = &line[..line.len() - 1];
            let valid = serde_json::from_slice::<EnvoyFrame>(record)
                .is_ok_and(|frame| event_identity(&frame).is_some());
            if !valid {
                break;
            }
            valid_len += read as u64;
        }
        if file.metadata()?.len() != valid_len {
            file.set_len(valid_len)?;
            file.seek(SeekFrom::Start(valid_len))?;
            file.sync_all()?;
        }
        Ok(())
    }

    fn path(&self, session_id: &str) -> PathBuf {
        self.dir.join(format!("{}.jsonl", encode_name(session_id)))
    }

    fn counter_path(&self, session_id: &str) -> PathBuf {
        self.dir.join(format!("{}.seq", encode_name(session_id)))
    }

    fn persist_counter(&self, session_id: &str, next: u64) -> Result<()> {
        let path = self.counter_path(session_id);
        let tmp = path.with_extension("seq.tmp");
        {
            let mut file = File::create(&tmp)?;
            writeln!(file, "{next}")?;
            file.sync_all()?;
        }
        fs::rename(tmp, path)?;
        sync_parent(&self.dir)
    }
}

fn event_identity(frame: &EnvoyFrame) -> Option<(&str, u64)> {
    match frame {
        EnvoyFrame::Event {
            session_id, seq, ..
        }
        | EnvoyFrame::Observed {
            session_id, seq, ..
        } => Some((session_id, *seq)),
        EnvoyFrame::JobOutput { job_id, seq, .. }
        | EnvoyFrame::JobResult { job_id, seq, .. } => Some((job_id, *seq)),
        _ => None,
    }
}

fn event_seq(frame: &EnvoyFrame) -> Option<u64> {
    event_identity(frame).map(|(_, seq)| seq)
}

fn encode_name(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_name(value: &str) -> Result<String> {
    if !value.len().is_multiple_of(2) {
        anyhow::bail!("invalid spool filename");
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    String::from_utf8(bytes).context("spool filename is not utf-8")
}

fn sync_parent(dir: &Path) -> Result<()> {
    File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use olympus_proto::agent::AgentEvent;

    use super::*;

    fn event(session: &str, seq: u64) -> EnvoyFrame {
        EnvoyFrame::Event {
            session_id: session.into(),
            turn_id: "turn-1".into(),
            seq,
            payload: AgentEvent::Text(format!("event-{seq}")),
        }
    }

    #[test]
    fn persists_replays_and_acknowledges_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let spool = EventSpool::open(dir.path()).unwrap();
        for seq in 0..4 {
            spool.append(&event("s/1", seq)).unwrap();
        }
        drop(spool);

        let reopened = EventSpool::open(dir.path()).unwrap();
        assert_eq!(reopened.last_seq("s/1").unwrap(), Some(3));
        assert_eq!(reopened.next_seq("s/1").unwrap(), 4);
        reopened.acknowledge("s/1", 1).unwrap();
        let replayed = reopened.read("s/1", Some(1)).unwrap();
        assert_eq!(
            replayed.iter().filter_map(event_seq).collect::<Vec<_>>(),
            [2, 3]
        );
        reopened.acknowledge("s/1", 3).unwrap();
        assert!(reopened.read("s/1", None).unwrap().is_empty());
        drop(reopened);
        let after_full_ack = EventSpool::open(dir.path()).unwrap();
        assert_eq!(after_full_ack.next_seq("s/1").unwrap(), 4);
    }

    #[test]
    fn truncated_tail_is_removed_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let spool = EventSpool::open(dir.path()).unwrap();
        spool.append(&event("s", 0)).unwrap();
        let path = spool.path("s");
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"kind\":")
            .unwrap();
        drop(spool);

        let reopened = EventSpool::open(dir.path()).unwrap();
        assert_eq!(reopened.read("s", None).unwrap(), vec![event("s", 0)]);
        assert_eq!(reopened.next_seq("s").unwrap(), 1);
    }

    #[test]
    fn corrupt_record_discards_invalid_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let spool = EventSpool::open(dir.path()).unwrap();
        spool.append(&event("s", 0)).unwrap();
        let path = spool.path("s");
        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(b"not-json\n").unwrap();
        serde_json::to_writer(&mut file, &event("s", 2)).unwrap();
        file.write_all(b"\n").unwrap();
        drop(spool);

        let reopened = EventSpool::open(dir.path()).unwrap();
        assert_eq!(reopened.read("s", None).unwrap(), vec![event("s", 0)]);
    }

    #[test]
    fn cap_fails_closed_without_partial_record() {
        let dir = tempfile::tempdir().unwrap();
        let spool = EventSpool::with_cap(dir.path(), 1).unwrap();
        let error = spool.append(&event("s", 0)).unwrap_err();
        assert!(error.to_string().contains("SPOOL_OVERFLOW"));
        assert!(spool.read("s", None).unwrap().is_empty());
    }
}
