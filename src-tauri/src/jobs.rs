//! Durable local job queue for asynchronous triggers (PRD §17.3).
//!
//! Jobs live in `<state>/data/jobs.db` (next to the global store), keyed by a
//! record-store identity (`studio:<documentId>` for Studio sessions,
//! `runtime:<bundleId>` for an installed runtime bundle, whose records are a
//! separate database), so they survive app restarts. The queue only runs while the app
//! is running: the frontend worker claims jobs, runs the action, and reports the
//! outcome. A claimed job holds a lease; a job whose lease expired (the app
//! crashed or quit mid-run) goes back to `queued` on the next start or claim.
//! Every claim issues a fresh lease token; completing or failing a job needs the
//! token of its current lease, so a stale worker (whose lease expired, or whose
//! job was cancelled and requeued) cannot finish someone else's attempt.
//! Lease tokens name the store instance (one per app process) that claimed the
//! job, so opening the store requeues jobs a previous process left running
//! without waiting for their lease to expire.
use crate::manager::AppError;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const DEFAULT_LEASE_MS: u64 = 5 * 60 * 1000;
/// Longest retry delay, however many attempts have failed.
const MAX_BACKOFF_MS: u64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub document_id: String,
    pub trigger_id: String,
    pub action_id: String,
    pub payload: Value,
    pub idempotency_key: String,
    pub status: String,
    pub attempts: u32,
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub next_run_at: String,
    pub lease_until: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_error: Option<String>,
    /// Token of the current lease while running (`<attempt>:<uuid>`).
    #[serde(default)]
    pub lease_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JobAttempt {
    pub job_id: String,
    pub attempt: u32,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub ok: Option<bool>,
    pub error: Option<String>,
    pub log: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueJob {
    pub trigger_id: String,
    pub action_id: String,
    #[serde(default)]
    pub payload: Value,
    pub idempotency_key: String,
    #[serde(default)]
    pub max_attempts: Option<u32>,
    #[serde(default)]
    pub backoff_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobFilter {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn after(now: DateTime<Utc>, ms: u64) -> String {
    ts(now + Duration::milliseconds(ms.min(i64::MAX as u64) as i64))
}
/// Delay before retry number `attempts` + 1: backoff · 2^(attempts − 1), capped.
pub fn backoff_delay(backoff_ms: u64, attempts: u32) -> u64 {
    let factor = 1u64 << attempts.saturating_sub(1).min(20);
    backoff_ms.saturating_mul(factor).min(MAX_BACKOFF_MS)
}

const COLUMNS: &str = "id,document_id,trigger_id,action_id,payload,idempotency_key,status,attempts,max_attempts,backoff_ms,next_run_at,lease_until,created_at,updated_at,last_error,lease_token";

/// `jobs` table (v2: `store_key` identity and `lease_token`). `{name}` is the table name.
const JOBS_TABLE: &str = "CREATE TABLE IF NOT EXISTS {name}(
   id TEXT PRIMARY KEY, document_id TEXT NOT NULL, trigger_id TEXT NOT NULL,
   action_id TEXT NOT NULL, payload TEXT NOT NULL, idempotency_key TEXT NOT NULL,
   status TEXT NOT NULL CHECK(status IN ('queued','running','succeeded','failed','cancelled')),
   attempts INTEGER NOT NULL DEFAULT 0, max_attempts INTEGER NOT NULL,
   backoff_ms INTEGER NOT NULL, next_run_at TEXT NOT NULL, lease_until TEXT,
   created_at TEXT NOT NULL, updated_at TEXT NOT NULL, last_error TEXT,
   store_key TEXT NOT NULL DEFAULT '', lease_token TEXT,
   UNIQUE(store_key, idempotency_key))";

fn job(r: &Row) -> rusqlite::Result<Job> {
    let payload: String = r.get(4)?;
    Ok(Job {
        id: r.get(0)?,
        document_id: r.get(1)?,
        trigger_id: r.get(2)?,
        action_id: r.get(3)?,
        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
        idempotency_key: r.get(5)?,
        status: r.get(6)?,
        attempts: r.get(7)?,
        max_attempts: r.get(8)?,
        backoff_ms: r.get::<_, i64>(9)? as u64,
        next_run_at: r.get(10)?,
        lease_until: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
        last_error: r.get(14)?,
        lease_token: r.get(15)?,
    })
}

fn err(e: impl ToString) -> AppError {
    AppError::new("JOB_STORE", e)
}

pub struct JobStore {
    path: PathBuf,
    /// This store instance (app process); part of every lease token it issues.
    instance: String,
}

impl JobStore {
    /// Opens (creating if needed) the queue database and recovers expired leases.
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        let store = Self {
            path: path.into(),
            instance: uuid::Uuid::new_v4().simple().to_string(),
        };
        let mut c = store.connection()?;
        c.execute_batch(&format!(
            "PRAGMA journal_mode=WAL; {};",
            JOBS_TABLE.replace("{name}", "jobs")
        ))
        .map_err(err)?;
        Self::migrate_v1(&mut c)?;
        c.execute_batch(
            "CREATE INDEX IF NOT EXISTS jobs_ready_v2 ON jobs(store_key, status, next_run_at);
             CREATE TABLE IF NOT EXISTS job_attempts(
               job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE, attempt INTEGER NOT NULL,
               started_at TEXT NOT NULL, finished_at TEXT, ok INTEGER, error TEXT, log TEXT,
               PRIMARY KEY(job_id, attempt));",
        )
        .map_err(err)?;
        store.recover_previous_instances(Utc::now())?;
        store.recover_expired(Utc::now())?;
        Ok(store)
    }

    /// Upgrades a v1 queue (keyed by document id, no lease tokens) in place. Old
    /// rows are kept and assigned to the Studio store of their document.
    fn migrate_v1(c: &mut Connection) -> Result<(), AppError> {
        let has_key = |c: &Connection| {
            c.query_row(
                "SELECT count(*) FROM pragma_table_info('jobs') WHERE name='store_key'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
        };
        if has_key(c).map_err(err)? {
            return Ok(());
        }
        // job_attempts references jobs(id): keep foreign keys off while the table is rebuilt.
        c.execute_batch("PRAGMA foreign_keys=OFF").map_err(err)?;
        let result = (|| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if !has_key(&tx)? {
                let old = "id,document_id,trigger_id,action_id,payload,idempotency_key,status,attempts,max_attempts,backoff_ms,next_run_at,lease_until,created_at,updated_at,last_error";
                tx.execute_batch(&format!(
                    "{}; INSERT INTO jobs_v2({old},store_key) SELECT {old},'studio:'||document_id FROM jobs;
                     DROP TABLE jobs; ALTER TABLE jobs_v2 RENAME TO jobs;",
                    JOBS_TABLE.replace("{name}", "jobs_v2")
                ))?;
            }
            tx.commit()
        })();
        c.execute_batch("PRAGMA foreign_keys=ON").map_err(err)?;
        result.map_err(err)
    }

    fn connection(&self) -> Result<Connection, AppError> {
        let c = Connection::open(&self.path).map_err(err)?;
        c.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(err)?;
        c.execute_batch("PRAGMA foreign_keys=ON;").map_err(err)?;
        Ok(c)
    }

    fn get(c: &Connection, document_id: &str, id: &str) -> Result<Job, AppError> {
        c.query_row(
            &format!("SELECT {COLUMNS} FROM jobs WHERE id=?1 AND store_key=?2"),
            params![id, document_id],
            job,
        )
        .optional()
        .map_err(err)?
        .ok_or_else(|| AppError::new("NOT_FOUND", format!("Job {id} not found")))
    }

    /// Adds a job to the queue `key` (also used as its document id), or returns
    /// the existing job with the same idempotency key.
    pub fn enqueue(
        &self,
        key: &str,
        req: &EnqueueJob,
        now: DateTime<Utc>,
    ) -> Result<Job, AppError> {
        self.enqueue_for(key, key, req, now)
    }

    /// Adds a job to the queue of store `key` for `document_id`, or returns the
    /// existing job with the same idempotency key in that queue.
    pub fn enqueue_for(
        &self,
        key: &str,
        document_id: &str,
        req: &EnqueueJob,
        now: DateTime<Utc>,
    ) -> Result<Job, AppError> {
        if req.idempotency_key.trim().is_empty() {
            return Err(AppError::new(
                "VALIDATION_ERROR",
                "Idempotency key is empty",
            ));
        }
        let c = self.connection()?;
        let id = uuid::Uuid::new_v4().to_string();
        let at = ts(now);
        c.execute(
            "INSERT INTO jobs(id,document_id,trigger_id,action_id,payload,idempotency_key,status,attempts,max_attempts,backoff_ms,next_run_at,created_at,updated_at,store_key)
             VALUES(?1,?2,?3,?4,?5,?6,'queued',0,?7,?8,?9,?9,?9,?10)
             ON CONFLICT(store_key, idempotency_key) DO NOTHING",
            params![
                id,
                document_id,
                req.trigger_id,
                req.action_id,
                req.payload.to_string(),
                req.idempotency_key,
                req.max_attempts.unwrap_or(3).max(1),
                req.backoff_ms.unwrap_or(1000) as i64,
                at,
                key
            ],
        )
        .map_err(err)?;
        c.query_row(
            &format!("SELECT {COLUMNS} FROM jobs WHERE store_key=?1 AND idempotency_key=?2"),
            params![key, req.idempotency_key],
            job,
        )
        .map_err(err)
    }

    /// Atomically claims the next due queued job: marks it running, counts the
    /// attempt, and opens an attempt record. Expired leases are recovered first.
    pub fn claim_next(
        &self,
        document_id: &str,
        now: DateTime<Utc>,
        lease_ms: u64,
    ) -> Result<Option<Job>, AppError> {
        self.recover_expired(now)?;
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let next: Option<String> = tx
            .query_row(
                "SELECT id FROM jobs WHERE store_key=?1 AND status='queued' AND next_run_at<=?2
                 ORDER BY next_run_at, created_at LIMIT 1",
                params![document_id, ts(now)],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        let Some(id) = next else {
            return Ok(None);
        };
        tx.execute(
            "UPDATE jobs SET status='running', attempts=attempts+1, lease_until=?2, updated_at=?3,
               lease_token=(attempts+1)||':'||?4||':'||?5 WHERE id=?1",
            params![
                id,
                after(now, lease_ms),
                ts(now),
                self.instance,
                uuid::Uuid::new_v4().to_string()
            ],
        )
        .map_err(err)?;
        tx.execute(
            "INSERT OR REPLACE INTO job_attempts(job_id,attempt,started_at) SELECT id,attempts,?2 FROM jobs WHERE id=?1",
            params![id, ts(now)],
        )
        .map_err(err)?;
        let claimed = Self::get(&tx, document_id, &id)?;
        tx.commit().map_err(err)?;
        Ok(Some(claimed))
    }

    fn finish_attempt(
        c: &Connection,
        j: &Job,
        now: DateTime<Utc>,
        ok: bool,
        error: Option<&str>,
        log: &Value,
    ) -> Result<(), AppError> {
        c.execute(
            "UPDATE job_attempts SET finished_at=?3, ok=?4, error=?5, log=?6 WHERE job_id=?1 AND attempt=?2",
            params![j.id, j.attempts, ts(now), ok, error, log.to_string()],
        )
        .map_err(err)?;
        Ok(())
    }

    /// The job, if `token` is its current lease; otherwise `STALE_LEASE` (the
    /// lease expired, or the job was cancelled, finished or claimed again).
    fn running(c: &Connection, document_id: &str, id: &str, token: &str) -> Result<Job, AppError> {
        let j = Self::get(c, document_id, id)?;
        if j.status != "running" || j.lease_token.as_deref() != Some(token) {
            return Err(AppError::new(
                "STALE_LEASE",
                format!(
                    "Job {id} is {} and this worker no longer holds its lease; the result was ignored",
                    j.status
                ),
            ));
        }
        Ok(j)
    }

    pub fn complete(
        &self,
        document_id: &str,
        id: &str,
        token: &str,
        log: &Value,
        now: DateTime<Utc>,
    ) -> Result<Job, AppError> {
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let j = Self::running(&tx, document_id, id, token)?;
        Self::finish_attempt(&tx, &j, now, true, None, log)?;
        tx.execute(
            "UPDATE jobs SET status='succeeded', lease_until=NULL, lease_token=NULL, last_error=NULL, updated_at=?2
             WHERE id=?1 AND status='running' AND lease_token=?3",
            params![id, ts(now), token],
        )
        .map_err(err)?;
        let done = Self::get(&tx, document_id, id)?;
        tx.commit().map_err(err)?;
        Ok(done)
    }

    /// Records a failed attempt; schedules a retry with exponential backoff, or
    /// marks the job failed once it has used all its attempts.
    pub fn fail(
        &self,
        document_id: &str,
        id: &str,
        token: &str,
        error: &str,
        log: &Value,
        now: DateTime<Utc>,
    ) -> Result<Job, AppError> {
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let j = Self::running(&tx, document_id, id, token)?;
        Self::finish_attempt(&tx, &j, now, false, Some(error), log)?;
        let (status, next) = if j.attempts >= j.max_attempts {
            ("failed", j.next_run_at.clone())
        } else {
            (
                "queued",
                after(now, backoff_delay(j.backoff_ms, j.attempts)),
            )
        };
        tx.execute(
            "UPDATE jobs SET status=?2, lease_until=NULL, lease_token=NULL, last_error=?3, updated_at=?4, next_run_at=?5
             WHERE id=?1 AND status='running' AND lease_token=?6",
            params![id, status, error, ts(now), next, token],
        )
        .map_err(err)?;
        let done = Self::get(&tx, document_id, id)?;
        tx.commit().map_err(err)?;
        Ok(done)
    }

    /// Cancels a queued or running job. A running attempt's later result is refused.
    /// Runs in one `IMMEDIATE` transaction with a status guard, so it cannot
    /// overwrite a result that a worker reports at the same moment.
    pub fn cancel(&self, document_id: &str, id: &str, now: DateTime<Utc>) -> Result<Job, AppError> {
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let j = Self::get(&tx, document_id, id)?;
        if !matches!(j.status.as_str(), "queued" | "running") {
            return Err(AppError::new(
                "INVALID_STATE",
                format!("Job {id} is already {}", j.status),
            ));
        }
        if j.status == "running" {
            Self::finish_attempt(&tx, &j, now, false, Some("Cancelled"), &Value::Null)?;
        }
        tx.execute(
            "UPDATE jobs SET status='cancelled', lease_until=NULL, lease_token=NULL, updated_at=?2
             WHERE id=?1 AND status IN ('queued','running')",
            params![id, ts(now)],
        )
        .map_err(err)?;
        let done = Self::get(&tx, document_id, id)?;
        tx.commit().map_err(err)?;
        Ok(done)
    }

    /// Requeues a failed or cancelled job to run now, allowing at least one more attempt.
    pub fn retry(&self, document_id: &str, id: &str, now: DateTime<Utc>) -> Result<Job, AppError> {
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let j = Self::get(&tx, document_id, id)?;
        if !matches!(j.status.as_str(), "failed" | "cancelled") {
            return Err(AppError::new(
                "INVALID_STATE",
                format!(
                    "Only failed or cancelled jobs can be retried; job {id} is {}",
                    j.status
                ),
            ));
        }
        tx.execute(
            "UPDATE jobs SET status='queued', next_run_at=?2, updated_at=?2, max_attempts=MAX(max_attempts, attempts+1)
             WHERE id=?1 AND status IN ('failed','cancelled')",
            params![id, ts(now)],
        )
        .map_err(err)?;
        let done = Self::get(&tx, document_id, id)?;
        tx.commit().map_err(err)?;
        Ok(done)
    }

    pub fn list(&self, document_id: &str, filter: &JobFilter) -> Result<Vec<Job>, AppError> {
        let c = self.connection()?;
        let mut s = c
            .prepare(&format!(
                "SELECT {COLUMNS} FROM jobs WHERE store_key=?1 AND (?2 IS NULL OR status=?2)
                 ORDER BY created_at DESC, rowid DESC LIMIT ?3"
            ))
            .map_err(err)?;
        let rows = s
            .query_map(
                params![document_id, filter.status, filter.limit.unwrap_or(200)],
                job,
            )
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    pub fn attempts(&self, document_id: &str, id: &str) -> Result<Vec<JobAttempt>, AppError> {
        let c = self.connection()?;
        Self::get(&c, document_id, id)?;
        let mut s = c
            .prepare("SELECT job_id,attempt,started_at,finished_at,ok,error,log FROM job_attempts WHERE job_id=?1 ORDER BY attempt")
            .map_err(err)?;
        let rows = s
            .query_map([id], |r| {
                let log: Option<String> = r.get(6)?;
                Ok(JobAttempt {
                    job_id: r.get(0)?,
                    attempt: r.get(1)?,
                    started_at: r.get(2)?,
                    finished_at: r.get(3)?,
                    ok: r.get(4)?,
                    error: r.get(5)?,
                    log: log
                        .and_then(|l| serde_json::from_str(&l).ok())
                        .unwrap_or(Value::Null),
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    /// The job `id` of queue `document_id` while it runs under `lease_token`
    /// with an unexpired lease (trigger writes made on the job's behalf).
    pub fn active_lease(
        &self,
        document_id: &str,
        id: &str,
        lease_token: &str,
        now: DateTime<Utc>,
    ) -> Result<Job, AppError> {
        let job = Self::get(&self.connection()?, document_id, id)?;
        let live = job.status == "running"
            && job.lease_token.as_deref() == Some(lease_token)
            && job
                .lease_until
                .as_deref()
                .is_some_and(|until| until > ts(now).as_str());
        if live {
            Ok(job)
        } else {
            Err(AppError::new(
                "STALE_LEASE",
                format!("Job {id} is not running under this lease"),
            ))
        }
    }

    /// Crash recovery: running jobs whose lease expired go back to queued (or to
    /// failed when they have no attempts left). Returns how many were recovered.
    pub fn recover_expired(&self, now: DateTime<Utc>) -> Result<usize, AppError> {
        self.requeue_running(
            now,
            "j.lease_until<?1",
            "Lease expired: the app stopped while the job was running",
        )
    }

    /// Startup recovery: running jobs leased by another store instance (an earlier
    /// app process; the app runs as a single instance) go back to queued at once
    /// instead of waiting for their lease to expire. Their old lease tokens stay
    /// refused (`STALE_LEASE`).
    fn recover_previous_instances(&self, now: DateTime<Utc>) -> Result<usize, AppError> {
        let mine = format!("%:{}:%", self.instance);
        self.requeue_running(
            now,
            &format!("coalesce(j.lease_token,'') NOT LIKE '{mine}'"),
            "The app restarted while the job was running",
        )
    }

    /// Requeues (or fails, when out of attempts) running jobs `j` matching `cond`
    /// (SQL; `?1` is now), closing their open attempt with `reason`.
    fn requeue_running(
        &self,
        now: DateTime<Utc>,
        cond: &str,
        reason: &str,
    ) -> Result<usize, AppError> {
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let at = ts(now);
        tx.execute(
            &format!(
                "UPDATE job_attempts SET finished_at=?1, ok=0, error=?2
                 WHERE finished_at IS NULL AND EXISTS(SELECT 1 FROM jobs j WHERE j.id=job_id AND j.attempts=attempt
                   AND j.status='running' AND {cond})"
            ),
            params![at, reason],
        )
        .map_err(err)?;
        let n = tx
            .execute(
                &format!(
                    "UPDATE jobs AS j SET status=CASE WHEN attempts>=max_attempts THEN 'failed' ELSE 'queued' END,
                       lease_until=NULL, lease_token=NULL, last_error=?2, next_run_at=?1, updated_at=?1
                     WHERE status='running' AND {cond}"
                ),
                params![at, reason],
            )
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(n)
    }
}

static STORE: OnceLock<JobStore> = OnceLock::new();
static STORE_INIT: Mutex<()> = Mutex::new(());

/// The value in `cell`, running `init` exactly once even under concurrent first
/// calls (a failed `init` leaves the cell empty for the next call to retry).
fn get_or_init_once<'a, T>(
    cell: &'a OnceLock<T>,
    lock: &Mutex<()>,
    init: impl FnOnce() -> Result<T, AppError>,
) -> Result<&'a T, AppError> {
    if let Some(v) = cell.get() {
        return Ok(v);
    }
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = cell.get() {
        return Ok(v);
    }
    let value = init()?;
    Ok(cell.get_or_init(|| value))
}

/// The app-wide queue in `<state>/data/jobs.db` (same state dir as the global store).
/// Opened once: a second open would run startup recovery again and requeue jobs
/// the first instance already claimed.
pub fn store() -> Result<&'static JobStore, AppError> {
    get_or_init_once(&STORE, &STORE_INIT, || {
        let base = crate::paths::state_dir();
        JobStore::open(&base.join("data").join("jobs.db"))
    })
}

/// The queue of the record store a window writes to: Studio sessions of a
/// document share `studio:<documentId>`; a runtime installation (its own
/// data.db, same document id) uses `runtime:<bundleId>`.
pub fn store_key(session_id: &str, document_id: &str) -> String {
    match crate::installation::runtime_session(session_id) {
        Some(runtime) => format!("runtime:{}", runtime.bundle_id),
        None => format!("studio:{document_id}"),
    }
}

pub fn document(window: &str) -> Result<String, AppError> {
    let state = crate::manager()?.state(window)?;
    Ok(store_key(&state.session_id, &state.document_id))
}

pub fn enqueue_job(window_label: String, job: crate::jobs::EnqueueJob) -> Result<Job, AppError> {
    let state = crate::manager()?.state(&window_label)?;
    let key = store_key(&state.session_id, &state.document_id);
    store()?.enqueue_for(&key, &state.document_id, &job, Utc::now())
}
pub fn claim_next_job(
    window_label: String,
    lease_ms: Option<u64>,
) -> Result<Option<Job>, AppError> {
    store()?.claim_next(
        &document(&window_label)?,
        Utc::now(),
        lease_ms.unwrap_or(DEFAULT_LEASE_MS),
    )
}
pub fn complete_job(
    window_label: String,
    id: String,
    lease_token: String,
    log: Option<Value>,
) -> Result<Job, AppError> {
    store()?.complete(
        &document(&window_label)?,
        &id,
        &lease_token,
        &log.unwrap_or(Value::Null),
        Utc::now(),
    )
}
pub fn fail_job(
    window_label: String,
    id: String,
    lease_token: String,
    error: String,
    log: Option<Value>,
) -> Result<Job, AppError> {
    store()?.fail(
        &document(&window_label)?,
        &id,
        &lease_token,
        &error,
        &log.unwrap_or(Value::Null),
        Utc::now(),
    )
}
pub fn cancel_job(window_label: String, id: String) -> Result<Job, AppError> {
    store()?.cancel(&document(&window_label)?, &id, Utc::now())
}
pub fn retry_job(window_label: String, id: String) -> Result<Job, AppError> {
    store()?.retry(&document(&window_label)?, &id, Utc::now())
}
pub fn list_jobs(
    window_label: String,
    filter: Option<crate::jobs::JobFilter>,
) -> Result<Vec<Job>, AppError> {
    store()?.list(&document(&window_label)?, &filter.unwrap_or_default())
}
pub fn job_attempts(window_label: String, id: String) -> Result<Vec<JobAttempt>, AppError> {
    store()?.attempts(&document(&window_label)?, &id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ixtable-jobs-{}", uuid::Uuid::new_v4()));
        dir.join("jobs.db")
    }
    fn req(key: &str) -> EnqueueJob {
        EnqueueJob {
            trigger_id: "t1".into(),
            action_id: "a1".into(),
            payload: serde_json::json!({"record": {"id": 1}}),
            idempotency_key: key.into(),
            max_attempts: Some(3),
            backoff_ms: Some(1000),
        }
    }
    impl JobStore {
        fn get_job(&self, document_id: &str, id: &str) -> Job {
            Self::get(&self.connection().unwrap(), document_id, id).unwrap()
        }
    }
    fn t0() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn enqueue_is_idempotent_per_document() {
        let s = JobStore::open(&temp()).unwrap();
        let a = s.enqueue("doc", &req("k1"), t0()).unwrap();
        let b = s.enqueue("doc", &req("k1"), t0()).unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(s.list("doc", &JobFilter::default()).unwrap().len(), 1);
        let other = s.enqueue("doc2", &req("k1"), t0()).unwrap();
        assert_ne!(other.id, a.id);
        assert!(s.enqueue("doc", &req(" "), t0()).is_err());
    }

    #[test]
    fn claim_is_atomic_and_exclusive() {
        let path = temp();
        let s = JobStore::open(&path).unwrap();
        for i in 0..5 {
            s.enqueue("doc", &req(&format!("k{i}")), t0()).unwrap();
        }
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let s = JobStore {
                        path,
                        instance: "worker".into(),
                    };
                    let mut mine = vec![];
                    while let Some(j) = s.claim_next("doc", t0(), 60_000).unwrap() {
                        mine.push(j.id);
                    }
                    mine
                })
            })
            .collect();
        let mut all: Vec<String> = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect();
        assert_eq!(all.len(), 5);
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 5, "a job was claimed twice");
        let running = s
            .list(
                "doc",
                &JobFilter {
                    status: Some("running".into()),
                    limit: None,
                },
            )
            .unwrap();
        assert_eq!(running.len(), 5);
        assert!(running.iter().all(|j| j.attempts == 1));
        assert!(s.claim_next("other-doc", t0(), 1).unwrap().is_none());
    }

    #[test]
    fn failure_retries_with_exponential_backoff_until_failed() {
        let s = JobStore::open(&temp()).unwrap();
        let j = s.enqueue("doc", &req("k"), t0()).unwrap();
        let mut now = t0();
        let mut delays = vec![];
        for attempt in 1..=3 {
            let claimed = s.claim_next("doc", now, 60_000).unwrap().unwrap();
            assert_eq!(claimed.attempts, attempt);
            let token = claimed.lease_token.clone().unwrap();
            let failed = s
                .fail(
                    "doc",
                    &j.id,
                    &token,
                    &format!("boom {attempt}"),
                    &serde_json::json!([]),
                    now,
                )
                .unwrap();
            if attempt < 3 {
                assert_eq!(failed.status, "queued");
                let next = DateTime::parse_from_rfc3339(&failed.next_run_at).unwrap();
                delays.push((next.with_timezone(&Utc) - now).num_milliseconds());
                // Not due before the backoff elapses.
                assert!(s.claim_next("doc", now, 60_000).unwrap().is_none());
                now = next.with_timezone(&Utc);
            } else {
                assert_eq!(failed.status, "failed");
                assert_eq!(failed.last_error.as_deref(), Some("boom 3"));
            }
        }
        assert_eq!(delays, vec![1000, 2000]);
        let history = s.attempts("doc", &j.id).unwrap();
        assert_eq!(history.len(), 3);
        assert!(history
            .iter()
            .all(|a| a.ok == Some(false) && a.finished_at.is_some()));
        assert_eq!(backoff_delay(1000, 30), MAX_BACKOFF_MS);
    }

    #[test]
    fn active_lease_accepts_only_the_current_unexpired_lease() {
        let s = JobStore::open(&temp()).unwrap();
        let j = s.enqueue("doc", &req("k"), t0()).unwrap();
        assert!(s.active_lease("doc", &j.id, "1:x", t0()).is_err());
        let claimed = s.claim_next("doc", t0(), 60_000).unwrap().unwrap();
        let token = claimed.lease_token.clone().unwrap();
        assert_eq!(
            s.active_lease("doc", &j.id, &token, t0())
                .unwrap()
                .trigger_id,
            "t1"
        );
        assert!(s.active_lease("doc", &j.id, "1:forged", t0()).is_err());
        assert!(s.active_lease("other", &j.id, &token, t0()).is_err());
        let late = t0() + Duration::milliseconds(61_000);
        assert!(s.active_lease("doc", &j.id, &token, late).is_err());
        s.complete("doc", &j.id, &token, &Value::Null, t0())
            .unwrap();
        assert!(s.active_lease("doc", &j.id, &token, t0()).is_err());
    }

    #[test]
    fn complete_records_success_and_log() {
        let s = JobStore::open(&temp()).unwrap();
        let j = s.enqueue("doc", &req("k"), t0()).unwrap();
        assert!(
            s.complete("doc", &j.id, "1:x", &Value::Null, t0()).is_err(),
            "not running"
        );
        let claimed = s.claim_next("doc", t0(), 60_000).unwrap().unwrap();
        let token = claimed.lease_token.unwrap();
        assert!(token.starts_with("1:"), "{token}");
        let done = s
            .complete(
                "doc",
                &j.id,
                &token,
                &serde_json::json!([{"ok": true}]),
                t0(),
            )
            .unwrap();
        assert_eq!(done.status, "succeeded");
        let history = s.attempts("doc", &j.id).unwrap();
        assert_eq!(history[0].ok, Some(true));
        assert_eq!(history[0].log, serde_json::json!([{"ok": true}]));
    }

    #[test]
    fn cancel_and_retry() {
        let s = JobStore::open(&temp()).unwrap();
        let j = s.enqueue("doc", &req("k"), t0()).unwrap();
        assert_eq!(s.cancel("doc", &j.id, t0()).unwrap().status, "cancelled");
        assert!(s.claim_next("doc", t0(), 60_000).unwrap().is_none());
        assert!(s.cancel("doc", &j.id, t0()).is_err());
        assert_eq!(s.retry("doc", &j.id, t0()).unwrap().status, "queued");
        let running = s.claim_next("doc", t0(), 60_000).unwrap().unwrap();
        assert!(
            s.retry("doc", &j.id, t0()).is_err(),
            "running jobs cannot be retried"
        );
        // Cancelling a running job refuses its later result.
        s.cancel("doc", &running.id, t0()).unwrap();
        let token = running.lease_token.clone().unwrap();
        assert!(s
            .complete("doc", &running.id, &token, &Value::Null, t0())
            .is_err());
        // Retry after exhausting attempts grants one more.
        let k = s
            .enqueue(
                "doc",
                &EnqueueJob {
                    max_attempts: Some(1),
                    ..req("k2")
                },
                t0(),
            )
            .unwrap();
        let token = s
            .claim_next("doc", t0(), 60_000)
            .unwrap()
            .unwrap()
            .lease_token
            .unwrap();
        assert_eq!(
            s.fail("doc", &k.id, &token, "x", &Value::Null, t0())
                .unwrap()
                .status,
            "failed"
        );
        let again = s.retry("doc", &k.id, t0()).unwrap();
        assert_eq!((again.status.as_str(), again.max_attempts), ("queued", 2));
        assert!(s.cancel("doc", "missing", t0()).is_err());
    }

    #[test]
    fn expired_leases_recover_after_restart() {
        let path = temp();
        {
            let s = JobStore::open(&path).unwrap();
            s.enqueue("doc", &req("k"), t0()).unwrap();
            s.enqueue("doc", &req("k-last"), t0()).unwrap();
            // Claim with a lease already in the past relative to "now" on restart.
            s.claim_next("doc", t0(), 1000).unwrap().unwrap();
            let last = s.claim_next("doc", t0(), 1000).unwrap().unwrap();
            // Make the second job's attempt its last one.
            Connection::open(&path)
                .unwrap()
                .execute("UPDATE jobs SET max_attempts=1 WHERE id=?1", [&last.id])
                .unwrap();
        }
        // "Restart": a new store on the same file recovers expired leases on open.
        let s = JobStore::open(&path).unwrap();
        let jobs = s.list("doc", &JobFilter::default()).unwrap();
        let by_key = |k: &str| jobs.iter().find(|j| j.idempotency_key == k).unwrap();
        assert_eq!(by_key("k").status, "queued");
        assert_eq!(by_key("k-last").status, "failed");
        assert!(by_key("k")
            .last_error
            .as_deref()
            .unwrap()
            .contains("restarted"));
        let history = s.attempts("doc", &by_key("k").id).unwrap();
        assert_eq!(history[0].ok, Some(false));
        // A lease that has not expired is left alone.
        let claimed = s.claim_next("doc", Utc::now(), 60_000).unwrap().unwrap();
        assert_eq!(claimed.attempts, 2);
        assert_eq!(s.recover_expired(Utc::now()).unwrap(), 0);
    }

    #[test]
    fn stale_workers_cannot_finish_a_requeued_job() {
        let s = JobStore::open(&temp()).unwrap();
        let j = s.enqueue("doc", &req("k"), t0()).unwrap();
        // Worker A claims, the job is cancelled and requeued, worker B claims it.
        let a = s.claim_next("doc", t0(), 60_000).unwrap().unwrap();
        s.cancel("doc", &j.id, t0()).unwrap();
        s.retry("doc", &j.id, t0()).unwrap();
        let b = s.claim_next("doc", t0(), 60_000).unwrap().unwrap();
        let (ta, tb) = (a.lease_token.unwrap(), b.lease_token.unwrap());
        assert_ne!(ta, tb);
        // A's late result is refused and changes nothing.
        let stale = s
            .complete("doc", &j.id, &ta, &Value::Null, t0())
            .unwrap_err();
        assert_eq!(stale.code, "STALE_LEASE");
        assert_eq!(
            s.fail("doc", &j.id, &ta, "late", &Value::Null, t0())
                .unwrap_err()
                .code,
            "STALE_LEASE"
        );
        let job = s.list("doc", &JobFilter::default()).unwrap().remove(0);
        assert_eq!((job.status.as_str(), job.attempts), ("running", 2));
        // B holds the current lease.
        assert_eq!(
            s.complete("doc", &j.id, &tb, &Value::Null, t0())
                .unwrap()
                .status,
            "succeeded"
        );
        assert_eq!(
            s.complete("doc", &j.id, &tb, &Value::Null, t0())
                .unwrap_err()
                .code,
            "STALE_LEASE"
        );

        // An expired lease that was re-claimed rejects the first worker too.
        let k = s.enqueue("doc", &req("k2"), t0()).unwrap();
        let first = s.claim_next("doc", t0(), 1).unwrap().unwrap();
        let later = t0() + Duration::seconds(10);
        let second = s.claim_next("doc", later, 60_000).unwrap().unwrap();
        assert_eq!(second.id, k.id);
        assert_eq!(
            s.complete(
                "doc",
                &k.id,
                first.lease_token.as_deref().unwrap(),
                &Value::Null,
                later
            )
            .unwrap_err()
            .code,
            "STALE_LEASE"
        );
    }

    #[test]
    fn restart_reclaims_unexpired_leases_of_the_previous_process() {
        let path = temp();
        let (job, old_token) = {
            let s = JobStore::open(&path).unwrap();
            s.enqueue("doc", &req("k"), Utc::now()).unwrap();
            let j = s
                .claim_next("doc", Utc::now(), DEFAULT_LEASE_MS)
                .unwrap()
                .unwrap();
            assert!(j.lease_token.as_deref().unwrap().contains(&s.instance));
            // A lease of this instance survives a recovery pass while it is valid.
            assert_eq!(s.recover_expired(Utc::now()).unwrap(), 0);
            let token = j.lease_token.clone().unwrap();
            (j, token)
        };
        // A new process opens the store: the 5-minute lease is reclaimed at once.
        let s = JobStore::open(&path).unwrap();
        let requeued = s.get_job("doc", &job.id);
        assert_eq!(requeued.status, "queued");
        assert!(requeued.last_error.unwrap().contains("restarted"));
        let again = s.claim_next("doc", Utc::now(), 60_000).unwrap().unwrap();
        assert_eq!((again.id.as_str(), again.attempts), (job.id.as_str(), 2));
        // The previous process's late result stays refused.
        let late = s
            .complete("doc", &job.id, &old_token, &Value::Null, Utc::now())
            .unwrap_err();
        assert_eq!(late.code, "STALE_LEASE");
        // Jobs this instance holds are left alone by its own reopen check.
        assert_eq!(s.recover_previous_instances(Utc::now()).unwrap(), 0);
    }

    #[test]
    fn cancel_and_results_are_guarded_transitions() {
        let s = JobStore::open(&temp()).unwrap();
        // A finished job cannot be cancelled afterwards.
        let a = s.enqueue("doc", &req("a"), t0()).unwrap();
        let ta = s
            .claim_next("doc", t0(), 60_000)
            .unwrap()
            .unwrap()
            .lease_token
            .unwrap();
        s.complete("doc", &a.id, &ta, &Value::Null, t0()).unwrap();
        assert_eq!(
            s.cancel("doc", &a.id, t0()).unwrap_err().code,
            "INVALID_STATE"
        );
        assert_eq!(s.get_job("doc", &a.id).status, "succeeded");
        // A cancelled job refuses its late result.
        let b = s.enqueue("doc", &req("b"), t0()).unwrap();
        let tb = s
            .claim_next("doc", t0(), 60_000)
            .unwrap()
            .unwrap()
            .lease_token
            .unwrap();
        s.cancel("doc", &b.id, t0()).unwrap();
        let late = s
            .fail("doc", &b.id, &tb, "x", &Value::Null, t0())
            .unwrap_err();
        assert_eq!(late.code, "STALE_LEASE");
        assert_eq!(s.get_job("doc", &b.id).status, "cancelled");
        // Racing cancel and complete: exactly one wins, and the loser changes nothing.
        let path = s.path.clone();
        for i in 0..10 {
            let j = s.enqueue("doc", &req(&format!("race{i}")), t0()).unwrap();
            let token = s
                .claim_next("doc", t0(), 60_000)
                .unwrap()
                .unwrap()
                .lease_token
                .unwrap();
            let (p1, p2, id1, id2) = (path.clone(), path.clone(), j.id.clone(), j.id.clone());
            let other = |p: PathBuf| JobStore {
                path: p,
                instance: "other".into(),
            };
            let done = std::thread::spawn(move || {
                other(p1)
                    .complete("doc", &id1, &token, &Value::Null, t0())
                    .is_ok()
            });
            let cancelled = std::thread::spawn(move || other(p2).cancel("doc", &id2, t0()).is_ok());
            let (done, cancelled) = (done.join().unwrap(), cancelled.join().unwrap());
            assert!(done ^ cancelled, "exactly one transition wins");
            let status = s.get_job("doc", &j.id).status;
            assert_eq!(status, if done { "succeeded" } else { "cancelled" });
            let attempt = &s.attempts("doc", &j.id).unwrap()[0];
            assert_eq!(attempt.ok, Some(done));
        }
    }

    #[test]
    fn studio_and_runtime_queues_are_separate() {
        let s = JobStore::open(&temp()).unwrap();
        let studio = s.enqueue_for("studio:doc", "doc", &req("k"), t0()).unwrap();
        let runtime = s
            .enqueue_for("runtime:doc", "doc", &req("k"), t0())
            .unwrap();
        assert_ne!(studio.id, runtime.id);
        assert_eq!(studio.document_id, "doc");
        let claimed = s.claim_next("runtime:doc", t0(), 60_000).unwrap().unwrap();
        assert_eq!(claimed.id, runtime.id);
        assert!(s.claim_next("runtime:doc", t0(), 60_000).unwrap().is_none());
        assert!(
            s.cancel("runtime:doc", &studio.id, t0()).is_err(),
            "other queue"
        );
        assert_eq!(
            s.list("studio:doc", &JobFilter::default()).unwrap().len(),
            1
        );
        assert_eq!(store_key("no-such-session", "doc"), "studio:doc");
    }

    #[test]
    fn v1_queue_is_migrated_keeping_rows() {
        let path = temp();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE jobs(
               id TEXT PRIMARY KEY, document_id TEXT NOT NULL, trigger_id TEXT NOT NULL,
               action_id TEXT NOT NULL, payload TEXT NOT NULL, idempotency_key TEXT NOT NULL,
               status TEXT NOT NULL CHECK(status IN ('queued','running','succeeded','failed','cancelled')),
               attempts INTEGER NOT NULL DEFAULT 0, max_attempts INTEGER NOT NULL,
               backoff_ms INTEGER NOT NULL, next_run_at TEXT NOT NULL, lease_until TEXT,
               created_at TEXT NOT NULL, updated_at TEXT NOT NULL, last_error TEXT,
               UNIQUE(document_id, idempotency_key));
             CREATE INDEX jobs_ready ON jobs(document_id, status, next_run_at);
             CREATE TABLE job_attempts(
               job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE, attempt INTEGER NOT NULL,
               started_at TEXT NOT NULL, finished_at TEXT, ok INTEGER, error TEXT, log TEXT,
               PRIMARY KEY(job_id, attempt));
             INSERT INTO jobs VALUES('j1','doc','t','a','{}','k','queued',1,3,1000,'2026-01-01T00:00:00.000Z',NULL,'2026-01-01T00:00:00.000Z','2026-01-01T00:00:00.000Z',NULL);
             INSERT INTO job_attempts VALUES('j1',1,'2026-01-01T00:00:00.000Z','2026-01-01T00:00:01.000Z',0,'boom','null');",
        )
        .unwrap();
        drop(c);
        let s = JobStore::open(&path).unwrap();
        let jobs = s.list("studio:doc", &JobFilter::default()).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(
            (jobs[0].id.as_str(), jobs[0].document_id.as_str()),
            ("j1", "doc")
        );
        assert_eq!(
            s.attempts("studio:doc", "j1").unwrap().len(),
            1,
            "attempts kept"
        );
        // The same idempotency key may now exist once per store.
        let again = s.enqueue_for("studio:doc", "doc", &req("k"), t0()).unwrap();
        assert_eq!(again.id, "j1");
        assert_ne!(
            s.enqueue_for("runtime:doc", "doc", &req("k"), t0())
                .unwrap()
                .id,
            "j1"
        );
        // Reopening is a no-op.
        drop(s);
        assert_eq!(
            JobStore::open(&path)
                .unwrap()
                .list("studio:doc", &JobFilter::default())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn concurrent_first_calls_open_the_store_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Barrier};
        static CELL: OnceLock<JobStore> = OnceLock::new();
        static LOCK: Mutex<()> = Mutex::new(());
        static OPENS: AtomicUsize = AtomicUsize::new(0);
        let path = temp();
        let barrier = Arc::new(Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let barrier = barrier.clone();
                let path = path.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let store = get_or_init_once(&CELL, &LOCK, || {
                        OPENS.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        JobStore::open(&path)
                    })
                    .unwrap();
                    store as *const JobStore as usize
                })
            })
            .collect();
        let stores: Vec<usize> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(OPENS.load(Ordering::SeqCst), 1);
        assert!(stores.iter().all(|s| *s == stores[0]));
    }

    #[test]
    fn failed_init_leaves_the_cell_empty_for_a_retry() {
        static CELL: OnceLock<u32> = OnceLock::new();
        static LOCK: Mutex<()> = Mutex::new(());
        assert!(get_or_init_once(&CELL, &LOCK, || Err(AppError::new("X", "boom"))).is_err());
        assert_eq!(*get_or_init_once(&CELL, &LOCK, || Ok(7)).unwrap(), 7);
    }
}
