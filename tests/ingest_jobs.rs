use foia_search::{
    ingest::jobs::IngestionJobLease,
    store::{NewIngestionJob, SqliteStore, StoreError},
};

fn new_job(job_key: &str, source_id: &str) -> NewIngestionJob {
    NewIngestionJob {
        job_key: job_key.to_owned(),
        operation: "ingest".to_owned(),
        source: "cia".to_owned(),
        source_id: Some(source_id.to_owned()),
        target_url: None,
        next_action: "Queued for ingestion pipeline.".to_owned(),
    }
}

#[test]
fn claims_oldest_queued_job_with_a_lease() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    store
        .create_ingestion_job(&new_job("ingest:cia:CREST-001", "CREST-001"))
        .expect("create first job");
    store
        .create_ingestion_job(&new_job("ingest:cia:CREST-002", "CREST-002"))
        .expect("create second job");

    let claimed = store
        .claim_next_ingestion_job(&IngestionJobLease {
            owner: "worker-a".to_owned(),
            now: "2026-05-15T10:00:00.000Z".to_owned(),
            expires_at: "2026-05-15T10:05:00.000Z".to_owned(),
        })
        .expect("claim queued job")
        .expect("queued job should be claimed");

    assert_eq!(claimed.job_key, "ingest:cia:CREST-001");
    assert_eq!(claimed.status, "running");
    assert_eq!(claimed.attempts, 1);
    assert_eq!(claimed.lease_owner.as_deref(), Some("worker-a"));
    assert_eq!(
        claimed.lease_expires_at.as_deref(),
        Some("2026-05-15T10:05:00.000Z")
    );

    let stored = store
        .get_ingestion_job_by_key("ingest:cia:CREST-001")
        .expect("load claimed job");
    assert_eq!(stored.status, "running");
    assert_eq!(stored.progress, 0.0);
}

#[test]
fn interruption_preserves_resume_stage_and_progress() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    store
        .create_ingestion_job(&new_job("ingest:cia:CREST-003", "CREST-003"))
        .expect("create job");

    store
        .claim_ingestion_job(
            "ingest:cia:CREST-003",
            &IngestionJobLease {
                owner: "worker-a".to_owned(),
                now: "2026-05-15T10:00:00.000Z".to_owned(),
                expires_at: "2026-05-15T10:05:00.000Z".to_owned(),
            },
        )
        .expect("claim job");
    store
        .mark_ingestion_job_stage(
            "ingest:cia:CREST-003",
            "worker-a",
            "extracting_text",
            0.45,
            Some("Text extraction is in progress."),
        )
        .expect("mark stage");

    let interrupted = store
        .interrupt_ingestion_job(
            "ingest:cia:CREST-003",
            "worker-a",
            Some("worker shutting down"),
            Some("Resume from extracting_text."),
        )
        .expect("interrupt job");

    assert_eq!(interrupted.status, "interrupted");
    assert_eq!(interrupted.stage, "extracting_text");
    assert_eq!(interrupted.progress, 0.45);
    assert_eq!(interrupted.lease_owner, None);
    assert_eq!(interrupted.error.as_deref(), Some("worker shutting down"));
}

#[test]
fn specific_claim_is_idempotent_for_same_worker() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    store
        .create_ingestion_job(&new_job("ingest:cia:CREST-004", "CREST-004"))
        .expect("create job");
    let lease = IngestionJobLease {
        owner: "worker-a".to_owned(),
        now: "2026-05-15T10:00:00.000Z".to_owned(),
        expires_at: "2026-05-15T10:05:00.000Z".to_owned(),
    };

    let first = store
        .claim_ingestion_job("ingest:cia:CREST-004", &lease)
        .expect("claim job");
    let second = store
        .claim_ingestion_job("ingest:cia:CREST-004", &lease)
        .expect("repeat claim");

    assert_eq!(first.job_key, second.job_key);
    assert_eq!(second.status, "running");
    assert_eq!(second.attempts, 1);
    assert_eq!(second.lease_owner.as_deref(), Some("worker-a"));
}

#[test]
fn progress_is_monotonic_and_warnings_are_deduplicated() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    store
        .create_ingestion_job(&new_job("ingest:cia:CREST-005", "CREST-005"))
        .expect("create job");
    store
        .claim_ingestion_job(
            "ingest:cia:CREST-005",
            &IngestionJobLease {
                owner: "worker-a".to_owned(),
                now: "2026-05-15T10:00:00.000Z".to_owned(),
                expires_at: "2026-05-15T10:05:00.000Z".to_owned(),
            },
        )
        .expect("claim job");

    store
        .mark_ingestion_job_stage("ingest:cia:CREST-005", "worker-a", "downloaded", 0.60, None)
        .expect("mark progress");
    let stage = store
        .mark_ingestion_job_stage("ingest:cia:CREST-005", "worker-a", "downloaded", 0.30, None)
        .expect("repeat lower progress");
    assert_eq!(stage.progress, 0.60);

    store
        .record_ingestion_job_warning("ingest:cia:CREST-005", "OCR quality below threshold")
        .expect("record warning");
    let warned = store
        .record_ingestion_job_warning("ingest:cia:CREST-005", "OCR quality below threshold")
        .expect("record warning idempotently");
    assert_eq!(warned.warnings, vec!["OCR quality below threshold"]);

    let invalid = store
        .mark_ingestion_job_stage("ingest:cia:CREST-005", "worker-a", "indexing", 1.2, None)
        .expect_err("invalid progress should fail");
    assert!(matches!(
        invalid,
        StoreError::InvalidIngestionJobProgress(progress) if progress == 1.2
    ));
}

#[test]
fn failed_job_keeps_original_terminal_error_on_replay() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    store
        .create_ingestion_job(&new_job("ingest:cia:CREST-006", "CREST-006"))
        .expect("create job");
    store
        .claim_ingestion_job(
            "ingest:cia:CREST-006",
            &IngestionJobLease {
                owner: "worker-a".to_owned(),
                now: "2026-05-15T10:00:00.000Z".to_owned(),
                expires_at: "2026-05-15T10:05:00.000Z".to_owned(),
            },
        )
        .expect("claim job");

    let failed = store
        .fail_ingestion_job(
            "ingest:cia:CREST-006",
            "worker-a",
            "download failed",
            Some("Retry with force=false after cache validation."),
        )
        .expect("fail job");
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.error.as_deref(), Some("download failed"));
    assert_eq!(failed.lease_owner, None);

    let replayed = store
        .fail_ingestion_job(
            "ingest:cia:CREST-006",
            "worker-b",
            "different failure",
            Some("different action"),
        )
        .expect("replay terminal failure");

    assert_eq!(replayed.status, "failed");
    assert_eq!(replayed.error.as_deref(), Some("download failed"));
    assert_eq!(
        replayed.next_action.as_deref(),
        Some("Retry with force=false after cache validation.")
    );
}

#[test]
fn wrong_worker_cannot_complete_running_job() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    store
        .create_ingestion_job(&new_job("ingest:cia:CREST-007", "CREST-007"))
        .expect("create job");
    store
        .claim_ingestion_job(
            "ingest:cia:CREST-007",
            &IngestionJobLease {
                owner: "worker-a".to_owned(),
                now: "2026-05-15T10:00:00.000Z".to_owned(),
                expires_at: "2026-05-15T10:05:00.000Z".to_owned(),
            },
        )
        .expect("claim job");

    let error = store
        .complete_ingestion_job("ingest:cia:CREST-007", "worker-b")
        .expect_err("wrong worker should not complete");
    assert!(matches!(
        error,
        StoreError::InvalidIngestionJobState { job_key, .. }
            if job_key == "ingest:cia:CREST-007"
    ));

    let stored = store
        .get_ingestion_job_record("ingest:cia:CREST-007")
        .expect("load job");
    assert_eq!(stored.status, "running");
    assert_eq!(stored.lease_owner.as_deref(), Some("worker-a"));
}

#[test]
fn expired_running_lease_can_be_reclaimed_for_resume() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    store
        .create_ingestion_job(&new_job("ingest:cia:CREST-008", "CREST-008"))
        .expect("create job");
    store
        .claim_ingestion_job(
            "ingest:cia:CREST-008",
            &IngestionJobLease {
                owner: "worker-a".to_owned(),
                now: "2026-05-15T10:00:00.000Z".to_owned(),
                expires_at: "2026-05-15T10:05:00.000Z".to_owned(),
            },
        )
        .expect("claim job");
    store
        .mark_ingestion_job_stage(
            "ingest:cia:CREST-008",
            "worker-a",
            "indexing",
            0.80,
            Some("Indexing local chunks."),
        )
        .expect("mark progress");

    let reclaimed = store
        .claim_next_ingestion_job(&IngestionJobLease {
            owner: "worker-b".to_owned(),
            now: "2026-05-15T10:06:00.000Z".to_owned(),
            expires_at: "2026-05-15T10:11:00.000Z".to_owned(),
        })
        .expect("claim stale running job")
        .expect("stale job should be reclaimed");

    assert_eq!(reclaimed.job_key, "ingest:cia:CREST-008");
    assert_eq!(reclaimed.status, "running");
    assert_eq!(reclaimed.stage, "indexing");
    assert_eq!(reclaimed.progress, 0.80);
    assert_eq!(reclaimed.attempts, 2);
    assert_eq!(reclaimed.lease_owner.as_deref(), Some("worker-b"));
}

#[test]
fn completion_sets_success_and_clears_lease() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    store
        .create_ingestion_job(&new_job("ingest:cia:CREST-009", "CREST-009"))
        .expect("create job");
    store
        .claim_ingestion_job(
            "ingest:cia:CREST-009",
            &IngestionJobLease {
                owner: "worker-a".to_owned(),
                now: "2026-05-15T10:00:00.000Z".to_owned(),
                expires_at: "2026-05-15T10:05:00.000Z".to_owned(),
            },
        )
        .expect("claim job");

    let completed = store
        .complete_ingestion_job("ingest:cia:CREST-009", "worker-a")
        .expect("complete job");

    assert_eq!(completed.status, "succeeded");
    assert_eq!(completed.stage, "succeeded");
    assert_eq!(completed.progress, 1.0);
    assert_eq!(completed.lease_owner, None);
    assert_eq!(completed.lease_expires_at, None);
    assert_eq!(completed.error, None);
}
