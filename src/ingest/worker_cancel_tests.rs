use crate::ingest::{CancellationSignal, QueuedIngestionWorker};

#[test]
fn shutdown_requests_cancellation_before_join() {
    let temp = tempfile::tempdir().expect("tempdir");
    let handle = QueuedIngestionWorker::new(temp.path(), Vec::new()).spawn();
    let cancellation = handle.cancellation_token();

    assert!(!cancellation.is_cancelled());
    handle.shutdown();
    assert!(cancellation.is_cancelled());
}
