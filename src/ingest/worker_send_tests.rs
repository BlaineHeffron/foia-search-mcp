use crate::ingest::QueuedIngestionWorker;
use std::future::Future;

fn assert_send<T: Send>(_: T) {}

fn assert_future_send<T>(future: T)
where
    T: Future + Send,
{
    assert_send(future);
}

#[test]
fn queued_worker_run_once_future_is_send() {
    let temp = tempfile::tempdir().expect("tempdir");
    let worker = QueuedIngestionWorker::new(temp.path(), Vec::new());
    assert_future_send(worker.run_once());
}
