use crate::store::{SqliteStore, StoreError, StoreResult};
use rusqlite::{params, OptionalExtension};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestionJobLease {
    pub owner: String,
    pub now: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IngestionJobRecord {
    pub job_key: String,
    pub document_id: Option<i64>,
    pub source: String,
    pub source_id: Option<String>,
    pub target_url: Option<String>,
    pub status: String,
    pub stage: String,
    pub progress: f64,
    pub attempts: u32,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<String>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    pub next_action: Option<String>,
}

impl SqliteStore {
    pub fn claim_next_ingestion_job(
        &self,
        lease: &IngestionJobLease,
    ) -> StoreResult<Option<IngestionJobRecord>> {
        let next_action = format!(
            "Claimed by {} until {}; ingestion execution may resume from stage.",
            lease.owner, lease.expires_at
        );
        let claimed = self
            .connection()
            .query_row(
                "
                UPDATE ingestion_jobs
                SET status = 'running',
                    attempts = attempts + 1,
                    lease_owner = ?1,
                    lease_expires_at = ?2,
                    error = NULL,
                    next_action = ?3,
                    updated_at = ?4
                WHERE id = (
                    SELECT id
                    FROM ingestion_jobs
                    WHERE status IN ('queued', 'interrupted')
                       OR (status = 'running' AND lease_expires_at IS NOT NULL AND lease_expires_at <= ?4)
                    ORDER BY
                        CASE status
                            WHEN 'queued' THEN 0
                            WHEN 'interrupted' THEN 1
                            ELSE 2
                        END,
                        updated_at,
                        id
                    LIMIT 1
                )
                RETURNING job_key, document_id, source, source_id, target_url, status, stage, progress,
                          attempts, lease_owner, lease_expires_at, error, warnings, next_action
                ",
                params![lease.owner, lease.expires_at, next_action, lease.now],
                read_ingestion_job_record,
            )
            .optional()?;

        Ok(claimed)
    }

    pub fn claim_ingestion_job(
        &self,
        job_key: &str,
        lease: &IngestionJobLease,
    ) -> StoreResult<IngestionJobRecord> {
        let next_action = format!(
            "Claimed by {} until {}; ingestion execution may resume from stage.",
            lease.owner, lease.expires_at
        );
        let claimed = self
            .connection()
            .query_row(
                "
                UPDATE ingestion_jobs
                SET status = 'running',
                    attempts = CASE
                        WHEN status = 'running' AND lease_owner = ?2 THEN attempts
                        ELSE attempts + 1
                    END,
                    lease_owner = ?2,
                    lease_expires_at = ?3,
                    error = NULL,
                    next_action = ?4,
                    updated_at = ?5
                WHERE job_key = ?1
                  AND (
                    status IN ('queued', 'interrupted')
                    OR (status = 'running' AND lease_owner = ?2)
                    OR (status = 'running' AND lease_expires_at IS NOT NULL AND lease_expires_at <= ?5)
                  )
                RETURNING job_key, document_id, source, source_id, target_url, status, stage, progress,
                          attempts, lease_owner, lease_expires_at, error, warnings, next_action
                ",
                params![job_key, lease.owner, lease.expires_at, next_action, lease.now],
                read_ingestion_job_record,
            )
            .optional()?;

        match claimed {
            Some(job) => Ok(job),
            None => {
                let job = self.get_ingestion_job_record(job_key)?;
                Err(StoreError::InvalidIngestionJobState {
                    job_key: job.job_key,
                    message: format!("job with status '{}' cannot be claimed", job.status),
                })
            }
        }
    }

    pub fn mark_ingestion_job_stage(
        &self,
        job_key: &str,
        lease_owner: &str,
        stage: &str,
        progress: f64,
        next_action: Option<&str>,
    ) -> StoreResult<IngestionJobRecord> {
        validate_progress(progress)?;
        self.connection().execute(
            "
            UPDATE ingestion_jobs
            SET stage = ?3,
                progress = max(progress, ?4),
                next_action = ?5,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE job_key = ?1 AND status = 'running' AND lease_owner = ?2
            ",
            params![job_key, lease_owner, stage, progress, next_action],
        )?;
        self.require_updated_running_job(job_key, lease_owner)
    }

    pub fn record_ingestion_job_warning(
        &self,
        job_key: &str,
        lease_owner: &str,
        warning: &str,
    ) -> StoreResult<IngestionJobRecord> {
        let mut job = self.require_updated_running_job(job_key, lease_owner)?;
        if !job.warnings.iter().any(|stored| stored == warning) {
            job.warnings.push(warning.to_owned());
            let warnings_json =
                serde_json::to_string(&job.warnings).map_err(|err| StoreError::InvalidJson {
                    field: "warnings",
                    value: err.to_string(),
                })?;
            self.connection().execute(
                "
                UPDATE ingestion_jobs
                SET warnings = ?2,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                WHERE job_key = ?1
                  AND status = 'running'
                  AND lease_owner = ?3
                ",
                params![job_key, warnings_json, lease_owner],
            )?;
        }
        self.require_updated_running_job(job_key, lease_owner)
    }

    pub fn record_ingestion_job_error(
        &self,
        job_key: &str,
        lease_owner: &str,
        error: &str,
    ) -> StoreResult<IngestionJobRecord> {
        self.connection().execute(
            "
            UPDATE ingestion_jobs
            SET error = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE job_key = ?1
              AND status = 'running'
              AND lease_owner = ?3
            ",
            params![job_key, error, lease_owner],
        )?;
        self.require_updated_running_job(job_key, lease_owner)
    }

    pub fn link_ingestion_job_document(
        &self,
        job_key: &str,
        lease_owner: &str,
        document_key: &crate::store::DocumentKey,
    ) -> StoreResult<IngestionJobRecord> {
        self.connection().execute(
            "
            UPDATE ingestion_jobs
            SET document_id = (
                    SELECT id
                    FROM documents
                    WHERE document_key = ?3
                ),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE job_key = ?1
              AND status = 'running'
              AND lease_owner = ?2
            ",
            params![job_key, lease_owner, document_key.as_str()],
        )?;
        let job = self.require_updated_running_job(job_key, lease_owner)?;
        if job.document_id.is_some() {
            Ok(job)
        } else {
            Err(StoreError::MissingDocument(document_key.to_string()))
        }
    }

    pub fn complete_ingestion_job(
        &self,
        job_key: &str,
        lease_owner: &str,
    ) -> StoreResult<IngestionJobRecord> {
        self.finish_ingestion_job(
            job_key,
            lease_owner,
            FinishedJob {
                status: "succeeded",
                stage: Some("succeeded"),
                progress: Some(1.0),
                error: None,
                next_action: Some("Ingestion completed; local search index is ready."),
            },
        )
    }

    pub fn fail_ingestion_job(
        &self,
        job_key: &str,
        lease_owner: &str,
        error: &str,
        next_action: Option<&str>,
    ) -> StoreResult<IngestionJobRecord> {
        self.finish_ingestion_job(
            job_key,
            lease_owner,
            FinishedJob {
                status: "failed",
                stage: Some("failed"),
                progress: None,
                error: Some(error),
                next_action,
            },
        )
    }

    pub fn interrupt_ingestion_job(
        &self,
        job_key: &str,
        lease_owner: &str,
        error: Option<&str>,
        next_action: Option<&str>,
    ) -> StoreResult<IngestionJobRecord> {
        self.finish_ingestion_job(
            job_key,
            lease_owner,
            FinishedJob {
                status: "interrupted",
                stage: None,
                progress: None,
                error,
                next_action,
            },
        )
    }

    pub fn get_ingestion_job_record(&self, job_key: &str) -> StoreResult<IngestionJobRecord> {
        self.connection()
            .query_row(
                "
                SELECT job_key, document_id, source, source_id, target_url, status, stage, progress,
                       attempts, lease_owner, lease_expires_at, error, warnings, next_action
                FROM ingestion_jobs
                WHERE job_key = ?1
                ",
                [job_key],
                read_ingestion_job_record,
            )
            .optional()?
            .ok_or_else(|| StoreError::MissingIngestionJob(job_key.to_owned()))
    }

    fn require_updated_running_job(
        &self,
        job_key: &str,
        lease_owner: &str,
    ) -> StoreResult<IngestionJobRecord> {
        let job = self.get_ingestion_job_record(job_key)?;
        if job.status == "running" && job.lease_owner.as_deref() == Some(lease_owner) {
            Ok(job)
        } else {
            Err(StoreError::InvalidIngestionJobState {
                job_key: job.job_key,
                message: "job is not running under the requested lease".to_owned(),
            })
        }
    }

    fn finish_ingestion_job(
        &self,
        job_key: &str,
        lease_owner: &str,
        finished: FinishedJob<'_>,
    ) -> StoreResult<IngestionJobRecord> {
        let current = self.get_ingestion_job_record(job_key)?;
        if current.status == finished.status {
            return Ok(current);
        }

        self.connection().execute(
            "
            UPDATE ingestion_jobs
            SET status = ?3,
                stage = COALESCE(?4, stage),
                progress = COALESCE(?5, progress),
                lease_owner = NULL,
                lease_expires_at = NULL,
                error = ?6,
                next_action = ?7,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE job_key = ?1
              AND status = 'running'
              AND lease_owner = ?2
            ",
            params![
                job_key,
                lease_owner,
                finished.status,
                finished.stage,
                finished.progress,
                finished.error,
                finished.next_action,
            ],
        )?;
        let job = self.get_ingestion_job_record(job_key)?;
        if job.status == finished.status {
            Ok(job)
        } else {
            Err(StoreError::InvalidIngestionJobState {
                job_key: job.job_key,
                message: format!(
                    "job with status '{}' cannot transition to '{}'",
                    job.status, finished.status
                ),
            })
        }
    }
}

struct FinishedJob<'a> {
    status: &'static str,
    stage: Option<&'static str>,
    progress: Option<f64>,
    error: Option<&'a str>,
    next_action: Option<&'a str>,
}

fn validate_progress(progress: f64) -> StoreResult<()> {
    if (0.0..=1.0).contains(&progress) {
        Ok(())
    } else {
        Err(StoreError::InvalidIngestionJobProgress(progress))
    }
}

fn read_ingestion_job_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<IngestionJobRecord> {
    let attempts: i64 = row.get(8)?;
    let warnings_json: String = row.get(12)?;
    let warnings = serde_json::from_str::<Vec<String>>(&warnings_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, Box::new(err))
    })?;

    Ok(IngestionJobRecord {
        job_key: row.get(0)?,
        document_id: row.get(1)?,
        source: row.get(2)?,
        source_id: row.get(3)?,
        target_url: row.get(4)?,
        status: row.get(5)?,
        stage: row.get(6)?,
        progress: row.get(7)?,
        attempts: attempts.max(0) as u32,
        lease_owner: row.get(9)?,
        lease_expires_at: row.get(10)?,
        error: row.get(11)?,
        warnings,
        next_action: row.get(13)?,
    })
}
