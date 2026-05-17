use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub trait CancellationSignal: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

impl<T> CancellationSignal for &T
where
    T: CancellationSignal + ?Sized,
{
    fn is_cancelled(&self) -> bool {
        (*self).is_cancelled()
    }
}

#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    requested: Arc<AtomicBool>,
}

#[derive(Clone, Debug)]
pub struct CancellationHandle {
    requested: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NeverCancel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationCheckpoint {
    AfterClaim,
    AfterSourceResolution,
    AfterPlanning,
    BeforeDownload,
    AfterDownload,
    BeforeExtraction,
    DuringExtraction,
    BeforePersistence,
    BeforeAssetProvenanceWrite,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&self) -> CancellationHandle {
        CancellationHandle {
            requested: Arc::clone(&self.requested),
        }
    }

    pub fn cancel(&self) {
        self.requested.store(true, Ordering::Relaxed);
    }
}

impl CancellationHandle {
    pub fn cancel(&self) {
        self.requested.store(true, Ordering::Relaxed);
    }
}

impl CancellationSignal for CancellationToken {
    fn is_cancelled(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
    }
}

impl CancellationSignal for CancellationHandle {
    fn is_cancelled(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
    }
}

impl CancellationSignal for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

impl CancellationCheckpoint {
    pub fn next_action(self) -> &'static str {
        match self {
            Self::AfterClaim => {
                "Shutdown interrupted this job immediately after claim; resume by reclaiming the job."
            }
            Self::AfterSourceResolution => {
                "Shutdown interrupted this job after source resolution; resume by reclaiming the job."
            }
            Self::AfterPlanning => {
                "Shutdown interrupted this job after planning; resume by reclaiming the job."
            }
            Self::BeforeDownload => {
                "Shutdown interrupted this job before asset download; resume by reclaiming the job."
            }
            Self::AfterDownload => {
                "Shutdown interrupted this job after download; resume by reclaiming the job."
            }
            Self::BeforeExtraction => {
                "Shutdown interrupted this job before extraction; resume by reclaiming the job."
            }
            Self::DuringExtraction => {
                "Shutdown interrupted this job during extraction; resume by reclaiming the job."
            }
            Self::BeforePersistence => {
                "Shutdown interrupted this job before persistence; resume by reclaiming the job."
            }
            Self::BeforeAssetProvenanceWrite => {
                "Shutdown interrupted this job before final asset provenance write; resume by reclaiming the job."
            }
        }
    }
}

pub fn ensure_not_cancelled(
    cancellation: &dyn CancellationSignal,
    checkpoint: CancellationCheckpoint,
) -> Result<(), CancellationCheckpoint> {
    if cancellation.is_cancelled() {
        Err(checkpoint)
    } else {
        Ok(())
    }
}

impl fmt::Display for CancellationCheckpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AfterClaim => f.write_str("after claim"),
            Self::AfterSourceResolution => f.write_str("after source resolution"),
            Self::AfterPlanning => f.write_str("after planning"),
            Self::BeforeDownload => f.write_str("before download"),
            Self::AfterDownload => f.write_str("after download"),
            Self::BeforeExtraction => f.write_str("before extraction"),
            Self::DuringExtraction => f.write_str("during extraction"),
            Self::BeforePersistence => f.write_str("before persistence"),
            Self::BeforeAssetProvenanceWrite => f.write_str("before final asset provenance write"),
        }
    }
}
