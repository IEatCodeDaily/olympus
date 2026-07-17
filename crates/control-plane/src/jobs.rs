use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use olympus_proto::frames::{HallFrame, JobAttemptState, JobAttemptStatus, JobStream};
use serde::Serialize;

use crate::event::Event;
use crate::log::Log;

const MAX_RETAINED_COMPLETED_JOBS: usize = 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub job_id: String,
    pub attempt_epoch: u64,
    pub organization_id: String,
    pub initiating_principal: String,
    pub initiating_session: Option<String>,
    pub node_id: String,
    pub package_id: String,
    pub package_version: String,
    pub package_digest: String,
    pub activity: String,
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub env_allowlist: Vec<String>,
    pub timeout_secs: u64,
    pub max_output_bytes: u64,
    pub created_at: f64,
    pub updated_at: f64,
    pub status: String,
    pub terminal_reason: Option<String>,
    pub output: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
    pub timed_out: bool,
    pub cancelled: bool,
    pub final_sequence: Option<u64>,
}

#[derive(Clone)]
pub struct JobService {
    log: Arc<Log>,
    jobs: Arc<RwLock<HashMap<String, JobRecord>>>,
}

impl JobService {
    pub fn open(log: Arc<Log>) -> Result<Self> {
        let service = Self {
            log: log.clone(),
            jobs: Arc::new(RwLock::new(HashMap::new())),
        };
        let events = log.read_all()?;
        for (_, event) in &events {
            service.apply(event);
        }
        service.prune_completed(&events)?;
        // Hall restart destroys certainty until the owning Envoy reports inventory.
        let running: Vec<_> = service
            .jobs
            .read()
            .expect("jobs projection poisoned")
            .values()
            .filter(|job| job.status == "running" || job.status == "recovering")
            .map(|job| (job.job_id.clone(), job.attempt_epoch))
            .collect();
        for (job_id, attempt_epoch) in running {
            service.record_reconciliation(
                &job_id,
                attempt_epoch,
                "StepIndeterminate",
                None,
                Some("hall_restarted_before_envoy_reconciliation".into()),
            )?;
        }
        Ok(service)
    }

    pub fn create(&self, event: Event) -> Result<()> {
        self.log.append(&event)?;
        self.apply(&event);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<JobRecord> {
        self.jobs.read().ok()?.get(id).cloned()
    }

    pub fn persist_output(
        &self,
        job_id: &str,
        attempt_epoch: u64,
        seq: u64,
        stream: JobStream,
        data: String,
    ) -> Result<bool> {
        self.ensure_epoch(job_id, attempt_epoch)?;
        let event = Event::JobOutputPersisted {
            job_id: job_id.into(),
            attempt_epoch,
            seq,
            stream,
            data,
            persisted_at: now(),
        };
        let fresh = match self
            .log
            .append_envoy_event(&wire_id(job_id, attempt_epoch), seq, &event)
        {
            Ok(fresh) => fresh,
            Err(error) => {
                self.record_reconciliation(
                    job_id,
                    attempt_epoch,
                    "StepIndeterminate",
                    None,
                    Some("output_sequence_gap_or_persistence_failure".into()),
                )?;
                return Err(error);
            }
        };
        if fresh {
            self.apply(&event);
        }
        Ok(fresh)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn persist_result(
        &self,
        job_id: &str,
        attempt_epoch: u64,
        seq: u64,
        exit_code: Option<i32>,
        truncated: bool,
        timed_out: bool,
        cancelled: bool,
    ) -> Result<bool> {
        self.ensure_epoch(job_id, attempt_epoch)?;
        let reason = if timed_out {
            "timed_out"
        } else if cancelled {
            "cancelled"
        } else if exit_code == Some(0) {
            "succeeded"
        } else {
            "failed"
        };
        let event = Event::JobTerminal {
            job_id: job_id.into(),
            attempt_epoch,
            seq: Some(seq),
            exit_code,
            truncated,
            timed_out,
            cancelled,
            terminal_reason: reason.into(),
            completed_at: now(),
        };
        let fresh = match self
            .log
            .append_envoy_event(&wire_id(job_id, attempt_epoch), seq, &event)
        {
            Ok(fresh) => fresh,
            Err(error) => {
                self.record_reconciliation(
                    job_id,
                    attempt_epoch,
                    "StepIndeterminate",
                    None,
                    Some("terminal_sequence_gap_or_persistence_failure".into()),
                )?;
                return Err(error);
            }
        };
        if fresh {
            self.apply(&event);
        }
        Ok(fresh)
    }

    pub fn dispatch_failed(&self, job_id: &str, attempt_epoch: u64, reason: String) -> Result<()> {
        let event = Event::JobTerminal {
            job_id: job_id.into(),
            attempt_epoch,
            seq: None,
            exit_code: None,
            truncated: false,
            timed_out: false,
            cancelled: false,
            terminal_reason: reason,
            completed_at: now(),
        };
        self.create(event)
    }

    pub fn dispatch_indeterminate(
        &self,
        job_id: &str,
        attempt_epoch: u64,
        reason: String,
    ) -> Result<()> {
        self.record_reconciliation(
            job_id,
            attempt_epoch,
            "StepIndeterminate",
            None,
            Some(reason),
        )
    }

    pub fn pending_dispatches(
        &self,
        node_id: &str,
        attempts: &[JobAttemptStatus],
    ) -> Vec<HallFrame> {
        self.jobs
            .read()
            .expect("jobs projection poisoned")
            .values()
            .filter(|job| {
                job.node_id == node_id
                    && job.status == "StepIndeterminate"
                    && job.terminal_reason.as_deref().is_some_and(|reason| {
                        reason == "hall_restarted_before_envoy_reconciliation"
                            || reason == "envoy_did_not_report_attempt"
                            || reason == "dispatch_acknowledgement_unknown"
                    })
                    && !attempts.iter().any(|attempt| {
                        attempt.job_id == job.job_id && attempt.attempt_epoch == job.attempt_epoch
                    })
            })
            .map(dispatch_frame)
            .collect()
    }

    pub fn reconcile(&self, node_id: &str, attempts: &[JobAttemptStatus]) -> Result<()> {
        let candidates: Vec<_> = self
            .jobs
            .read()
            .expect("jobs projection poisoned")
            .values()
            .filter(|job| job.node_id == node_id && !is_terminal(&job.status))
            .map(|job| (job.job_id.clone(), job.attempt_epoch))
            .collect();
        for (job_id, epoch) in candidates {
            match attempts
                .iter()
                .find(|attempt| attempt.job_id == job_id && attempt.attempt_epoch == epoch)
            {
                Some(attempt) => {
                    let (status, reason) = match attempt.state {
                        JobAttemptState::Running => ("running", None),
                        JobAttemptState::Completed if attempt.final_sequence.is_some() => (
                            "recovering",
                            Some("replaying_durable_terminal_sequence".into()),
                        ),
                        JobAttemptState::Completed => (
                            "StepIndeterminate",
                            Some("envoy_completed_without_durable_terminal_sequence".into()),
                        ),
                        JobAttemptState::StepIndeterminate => {
                            ("StepIndeterminate", attempt.terminal_reason.clone())
                        }
                    };
                    self.record_reconciliation(&job_id, epoch, status, Some(attempt), reason)?;
                }
                None => self.record_reconciliation(
                    &job_id,
                    epoch,
                    "StepIndeterminate",
                    None,
                    Some("envoy_did_not_report_attempt".into()),
                )?,
            }
        }
        Ok(())
    }

    fn record_reconciliation(
        &self,
        job_id: &str,
        attempt_epoch: u64,
        status: &str,
        attempt: Option<&JobAttemptStatus>,
        terminal_reason: Option<String>,
    ) -> Result<()> {
        let exit_code = attempt.and_then(|attempt| attempt.exit_code);
        let truncated = attempt.is_some_and(|attempt| attempt.truncated);
        let timed_out = attempt.is_some_and(|attempt| attempt.timed_out);
        let cancelled = attempt.is_some_and(|attempt| attempt.cancelled);
        self.create(Event::JobReconciled {
            job_id: job_id.into(),
            attempt_epoch,
            status: status.into(),
            exit_code,
            truncated,
            timed_out,
            cancelled,
            terminal_reason,
            reconciled_at: now(),
        })
    }

    fn ensure_epoch(&self, job_id: &str, epoch: u64) -> Result<()> {
        let jobs = self.jobs.read().expect("jobs projection poisoned");
        let job = jobs
            .get(job_id)
            .with_context(|| format!("unknown job {job_id}"))?;
        anyhow::ensure!(job.attempt_epoch == epoch, "stale job attempt epoch");
        anyhow::ensure!(
            job.status != "StepIndeterminate",
            "job attempt is indeterminate"
        );
        Ok(())
    }

    fn prune_completed(&self, events: &[(u64, Event)]) -> Result<()> {
        let mut completed = self
            .jobs
            .read()
            .expect("jobs projection poisoned")
            .values()
            .filter(|job| job.status == "completed")
            .map(|job| (job.updated_at, job.job_id.clone(), job.attempt_epoch))
            .collect::<Vec<_>>();
        completed.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
        let excess = completed.len().saturating_sub(MAX_RETAINED_COMPLETED_JOBS);
        let removed = completed
            .into_iter()
            .take(excess)
            .map(|(_, id, epoch)| (id, epoch))
            .collect::<Vec<_>>();
        if removed.is_empty() {
            return Ok(());
        }
        let ids = removed
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<HashSet<_>>();
        let seqs = events
            .iter()
            .filter_map(|(seq, event)| {
                event_job_id(event)
                    .filter(|id| ids.contains(id))
                    .map(|_| *seq)
            })
            .collect::<Vec<_>>();
        let identities = removed
            .iter()
            .map(|(id, epoch)| wire_id(id, *epoch))
            .collect::<Vec<_>>();
        self.log.delete_job_history(&seqs, &identities)?;
        self.jobs
            .write()
            .expect("jobs projection poisoned")
            .retain(|id, _| !ids.contains(id.as_str()));
        Ok(())
    }

    fn apply(&self, event: &Event) {
        let mut jobs = self.jobs.write().expect("jobs projection poisoned");
        match event {
            Event::JobDispatchIntent {
                job_id,
                attempt_epoch,
                organization_id,
                initiating_principal,
                initiating_session,
                node_id,
                package_id,
                package_version,
                package_digest,
                activity,
                argv,
                cwd,
                env_allowlist,
                timeout_secs,
                max_output_bytes,
                created_at,
            } => {
                if jobs
                    .get(job_id)
                    .is_none_or(|job| *attempt_epoch > job.attempt_epoch)
                {
                    jobs.insert(
                        job_id.clone(),
                        JobRecord {
                            job_id: job_id.clone(),
                            attempt_epoch: *attempt_epoch,
                            organization_id: organization_id.clone(),
                            initiating_principal: initiating_principal.clone(),
                            initiating_session: initiating_session.clone(),
                            node_id: node_id.clone(),
                            package_id: package_id.clone(),
                            package_version: package_version.clone(),
                            package_digest: package_digest.clone(),
                            activity: activity.clone(),
                            argv: argv.clone(),
                            cwd: cwd.clone(),
                            env_allowlist: env_allowlist.clone(),
                            timeout_secs: *timeout_secs,
                            max_output_bytes: *max_output_bytes,
                            created_at: *created_at,
                            updated_at: *created_at,
                            status: "running".into(),
                            terminal_reason: None,
                            output: String::new(),
                            exit_code: None,
                            truncated: false,
                            timed_out: false,
                            cancelled: false,
                            final_sequence: None,
                        },
                    );
                }
            }
            Event::JobOutputPersisted {
                job_id,
                stream,
                data,
                persisted_at,
                ..
            } => {
                if let Some(job) = jobs.get_mut(job_id) {
                    if *stream == JobStream::Stderr {
                        job.output.push_str("[stderr] ");
                    }
                    job.output.push_str(data);
                    if job.output.len() > 65_536 {
                        job.output.drain(..job.output.len() - 65_536);
                    }
                    job.updated_at = *persisted_at;
                }
            }
            Event::JobTerminal {
                job_id,
                seq,
                exit_code,
                truncated,
                timed_out,
                cancelled,
                terminal_reason,
                completed_at,
                ..
            } => {
                if let Some(job) = jobs.get_mut(job_id) {
                    job.status = "completed".into();
                    job.exit_code = *exit_code;
                    job.truncated = *truncated;
                    job.timed_out = *timed_out;
                    job.cancelled = *cancelled;
                    job.terminal_reason = Some(terminal_reason.clone());
                    job.updated_at = *completed_at;
                    job.final_sequence = *seq;
                }
            }
            Event::JobReconciled {
                job_id,
                status,
                exit_code,
                truncated,
                timed_out,
                cancelled,
                terminal_reason,
                reconciled_at,
                ..
            } => {
                if let Some(job) = jobs.get_mut(job_id) {
                    job.status.clone_from(status);
                    job.exit_code = *exit_code;
                    job.truncated = *truncated;
                    job.timed_out = *timed_out;
                    job.cancelled = *cancelled;
                    job.terminal_reason.clone_from(terminal_reason);
                    job.updated_at = *reconciled_at;
                }
            }
            _ => {}
        }
    }
}

pub fn wire_id(job_id: &str, attempt_epoch: u64) -> String {
    format!("job:{job_id}:{attempt_epoch}")
}

fn is_terminal(status: &str) -> bool {
    status == "completed"
}

fn event_job_id(event: &Event) -> Option<&str> {
    match event {
        Event::JobDispatchIntent { job_id, .. }
        | Event::JobOutputPersisted { job_id, .. }
        | Event::JobTerminal { job_id, .. }
        | Event::JobReconciled { job_id, .. } => Some(job_id),
        _ => None,
    }
}

fn dispatch_frame(job: &JobRecord) -> HallFrame {
    HallFrame::DispatchJob {
        req_id: 0,
        job_id: job.job_id.clone(),
        attempt_epoch: job.attempt_epoch,
        package_id: job.package_id.clone(),
        package_version: job.package_version.clone(),
        package_digest: job.package_digest.clone(),
        activity: job.activity.clone(),
        argv: job.argv.clone(),
        env_allowlist: job.env_allowlist.clone(),
        cwd: job.cwd.clone(),
        timeout_secs: job.timeout_secs,
        max_output_bytes: job.max_output_bytes,
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dispatch(id: &str) -> Event {
        dispatch_epoch(id, 1)
    }

    fn dispatch_epoch(id: &str, attempt_epoch: u64) -> Event {
        Event::JobDispatchIntent {
            job_id: id.into(),
            attempt_epoch,
            organization_id: "o".into(),
            initiating_principal: "p".into(),
            initiating_session: None,
            node_id: "n".into(),
            package_id: "pkg".into(),
            package_version: "1".into(),
            package_digest: "d".into(),
            activity: "job.run".into(),
            argv: vec!["true".into()],
            cwd: None,
            env_allowlist: vec![],
            timeout_secs: 1,
            max_output_bytes: 10,
            created_at: 1.0,
        }
    }

    #[test]
    fn restart_reconciliation_duplicate_result_and_output_gap_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(Log::open(&dir.path().join("db")).unwrap());
        let jobs = JobService::open(log.clone()).unwrap();
        jobs.create(dispatch("done")).unwrap();
        jobs.persist_output("done", 1, 0, JobStream::Stdout, "a".into())
            .unwrap();
        assert!(jobs
            .persist_output("done", 1, 2, JobStream::Stdout, "gap".into())
            .is_err());
        assert_eq!(jobs.get("done").unwrap().status, "StepIndeterminate");
        assert_eq!(
            jobs.get("done").unwrap().terminal_reason.as_deref(),
            Some("output_sequence_gap_or_persistence_failure")
        );
        assert!(jobs
            .persist_result("done", 1, 1, Some(0), false, false, false)
            .is_err());

        jobs.create(dispatch("result")).unwrap();
        assert!(jobs
            .persist_result("result", 1, 0, Some(0), false, false, false)
            .unwrap());
        assert!(!jobs
            .persist_result("result", 1, 0, Some(0), false, false, false)
            .unwrap());

        jobs.create(dispatch("running")).unwrap();
        drop(jobs);
        let restarted = JobService::open(log).unwrap();
        assert_eq!(restarted.get("done").unwrap().output, "a");
        assert_eq!(
            restarted.get("running").unwrap().status,
            "StepIndeterminate"
        );
        assert_eq!(restarted.pending_dispatches("n", &[]).len(), 1);

        let running_attempt = JobAttemptStatus {
            job_id: "running".into(),
            attempt_epoch: 1,
            state: JobAttemptState::Running,
            exit_code: None,
            truncated: false,
            timed_out: false,
            cancelled: false,
            terminal_reason: None,
            final_sequence: None,
        };
        assert!(restarted
            .pending_dispatches("n", std::slice::from_ref(&running_attempt))
            .is_empty());
        restarted
            .reconcile("n", std::slice::from_ref(&running_attempt))
            .unwrap();
        assert_eq!(restarted.get("running").unwrap().status, "running");
        restarted.reconcile("n", &[]).unwrap();
        assert_eq!(
            restarted.get("running").unwrap().status,
            "StepIndeterminate"
        );

        restarted.create(dispatch("reported-complete")).unwrap();
        restarted
            .reconcile(
                "n",
                &[JobAttemptStatus {
                    job_id: "reported-complete".into(),
                    attempt_epoch: 1,
                    state: JobAttemptState::Completed,
                    exit_code: Some(0),
                    truncated: false,
                    timed_out: false,
                    cancelled: false,
                    terminal_reason: Some("succeeded".into()),
                    final_sequence: None,
                }],
            )
            .unwrap();
        let reported = restarted.get("reported-complete").unwrap();
        assert_eq!(reported.status, "StepIndeterminate");
        assert_eq!(
            reported.terminal_reason.as_deref(),
            Some("envoy_completed_without_durable_terminal_sequence")
        );
        assert_eq!(reported.final_sequence, None);
    }

    #[test]
    fn hall_restart_before_terminal_ack_replays_to_completion() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(Log::open(&dir.path().join("db")).unwrap());
        let jobs = JobService::open(log.clone()).unwrap();
        jobs.create(dispatch("recover")).unwrap();
        drop(jobs);

        let restarted = JobService::open(log.clone()).unwrap();
        let completed_attempt = JobAttemptStatus {
            job_id: "recover".into(),
            attempt_epoch: 1,
            state: JobAttemptState::Completed,
            exit_code: Some(0),
            truncated: false,
            timed_out: false,
            cancelled: false,
            terminal_reason: Some("succeeded".into()),
            final_sequence: Some(1),
        };
        restarted
            .reconcile("n", std::slice::from_ref(&completed_attempt))
            .unwrap();
        assert_eq!(restarted.get("recover").unwrap().status, "recovering");
        drop(restarted);

        let restarted = JobService::open(log).unwrap();
        assert_eq!(
            restarted.get("recover").unwrap().status,
            "StepIndeterminate"
        );
        restarted.reconcile("n", &[completed_attempt]).unwrap();
        assert_eq!(restarted.get("recover").unwrap().status, "recovering");
        restarted
            .persist_output("recover", 1, 0, JobStream::Stdout, "done".into())
            .unwrap();
        restarted
            .persist_result("recover", 1, 1, Some(0), false, false, false)
            .unwrap();
        let recovered = restarted.get("recover").unwrap();
        assert_eq!(recovered.status, "completed");
        assert_eq!(recovered.final_sequence, Some(1));
    }

    #[test]
    fn completed_job_history_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(Log::open(&dir.path().join("db")).unwrap());
        let jobs = JobService::open(log.clone()).unwrap();
        for index in 0..=1024 {
            let id = format!("retained-{index:04}");
            jobs.create(dispatch(&id)).unwrap();
            jobs.persist_result(&id, 1, 0, Some(0), false, false, false)
                .unwrap();
        }
        drop(jobs);

        let reopened = JobService::open(log.clone()).unwrap();
        assert!(reopened.get("retained-0000").is_none());
        assert!(reopened.get("retained-1024").is_some());
        assert!(log.event_count().unwrap() <= 2048);
    }

    #[test]
    fn newer_dispatch_intent_advances_epoch_and_fences_stale_output() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(Log::open(&dir.path().join("db")).unwrap());
        let jobs = JobService::open(log).unwrap();
        jobs.create(dispatch_epoch("retry", 1)).unwrap();
        jobs.create(dispatch_epoch("retry", 2)).unwrap();

        assert_eq!(jobs.get("retry").unwrap().attempt_epoch, 2);
        assert!(jobs
            .persist_output("retry", 1, 0, JobStream::Stdout, "stale".into())
            .is_err());
        assert!(jobs
            .persist_output("retry", 2, 0, JobStream::Stdout, "current".into())
            .unwrap());
    }
}
