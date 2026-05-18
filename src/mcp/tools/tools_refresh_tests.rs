use crate::{
    ingest::{OcrBackendConfig, OcrFallbackPolicy, QueuedIngestionWorker},
    store::{DocumentKey, SqliteStore, UpsertDocument},
};
use std::{sync::Arc, thread, time::Duration};

#[tokio::test]
async fn refresh_document_kicks_configured_worker_after_durable_enqueue() {
    let temp = tempfile::tempdir().expect("tempdir");
    seed_refresh_document(temp.path());
    let worker = QueuedIngestionWorker::new(temp.path(), Vec::new()).spawn();
    let kick = worker.kick_handle().expect("worker kick handle");
    let server =
        FoiaSearchServer::from_parts(Arc::new(test_config(temp.path())), Arc::new(Vec::new()))
            .with_ingestion_worker(kick);

    let response = server
        .refresh_document(Parameters(RefreshDocumentParams {
            document_id: "cia:CREST-refresh-tool".to_owned(),
            force: Some(false),
        }))
        .await
        .expect("refresh tool response");

    assert!(!response.content.is_empty());
    let job = wait_for_worker_attempt(temp.path());
    assert_eq!(job.status, "failed");
    assert_eq!(job.attempts, 1);
    assert!(job
        .error
        .as_deref()
        .expect("failed worker job should record an error")
        .contains("no source adapter registered"));

    worker.shutdown();
}

fn test_config(data_dir: &std::path::Path) -> Config {
    Config {
        data_dir: data_dir.to_owned(),
        nara_api_key: None,
        nara_api_base_url: "https://catalog.archives.gov/api/v2".to_owned(),
        ocr_fallback_policy: OcrFallbackPolicy::off(),
        ocr_backend: OcrBackendConfig::default(),
    }
}

fn seed_refresh_document(data_dir: &std::path::Path) {
    let db_dir = data_dir.join("db");
    std::fs::create_dir_all(&db_dir).expect("create db dir");
    let store = SqliteStore::open(db_dir.join("foia.sqlite")).expect("open store");
    let key = DocumentKey::new("doc_cia_refresh_tool").expect("safe fixture key");
    store
        .upsert_document(&UpsertDocument {
            public_id: "cia:CREST-refresh-tool".to_owned(),
            document_key: key,
            source: "cia".to_owned(),
            source_id: "CREST-refresh-tool".to_owned(),
            title: "Refresh Tool Fixture".to_owned(),
            date: None,
            collection: Some("CREST".to_owned()),
            record_group: None,
            description: Some("Refresh tool local document fixture".to_owned()),
            origin_url: Some(
                "https://www.cia.gov/readingroom/document/CREST-refresh-tool".to_owned(),
            ),
            document_url: Some(
                "https://www.cia.gov/readingroom/document/CREST-refresh-tool".to_owned(),
            ),
            pdf_url: Some("https://www.cia.gov/readingroom/docs/CREST-refresh-tool.pdf".to_owned()),
            metadata_json: "{}".to_owned(),
            citation_note: Some("Cite refreshed tool fixture pages.".to_owned()),
            terms_note: Some("Respect refreshed tool fixture terms.".to_owned()),
        })
        .expect("seed refresh tool document");
}

fn wait_for_worker_attempt(data_dir: &std::path::Path) -> crate::ingest::IngestionJobRecord {
    for _ in 0..40 {
        let store = open_store(data_dir);
        let job = store
            .get_ingestion_job_record("refresh:cia:CREST-refresh-tool")
            .expect("refresh job");
        if job.attempts > 0 {
            return job;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("worker did not claim kicked refresh job");
}

fn open_store(data_dir: &std::path::Path) -> SqliteStore {
    SqliteStore::open(data_dir.join("db").join("foia.sqlite")).expect("open store")
}
