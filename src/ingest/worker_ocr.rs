use crate::ingest::{
    ExtractedText, NoopOcrExtractor, OcrBackend, OcrBackendConfig, OcrFallbackPolicy,
    OcrmypdfConfig, OcrmypdfExtractor, TextExtraction, TextExtractor,
};
use std::path::Path;

enum WorkerOcrExtractor {
    Noop(NoopOcrExtractor),
    Ocrmypdf(OcrmypdfExtractor),
}

impl TextExtractor for WorkerOcrExtractor {
    fn extract_pages(&self, path: &Path) -> Result<ExtractedText, TextExtraction> {
        self.extract_pages_with_cancel(path, &|| false)
    }

    fn extract_pages_with_cancel(
        &self,
        path: &Path,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ExtractedText, TextExtraction> {
        match self {
            Self::Noop(extractor) => extractor.extract_pages(path),
            Self::Ocrmypdf(extractor) => extractor.extract_pages_with_cancel(path, is_cancelled),
        }
    }
}

pub(crate) fn worker_ocr_extractor(
    policy: OcrFallbackPolicy,
    backend_config: &OcrBackendConfig,
) -> impl TextExtractor {
    match effective_ocr_backend(policy, backend_config) {
        OcrBackend::Ocrmypdf => {
            WorkerOcrExtractor::Ocrmypdf(OcrmypdfExtractor::new(OcrmypdfConfig::new(
                backend_config.ocrmypdf_binary.clone(),
                backend_config.timeout,
                backend_config.max_stderr_bytes,
            )))
        }
        OcrBackend::None | OcrBackend::Tesseract => WorkerOcrExtractor::Noop(NoopOcrExtractor),
    }
}

fn effective_ocr_backend(
    policy: OcrFallbackPolicy,
    backend_config: &OcrBackendConfig,
) -> OcrBackend {
    if policy.is_enabled() && backend_config.backend.is_enabled() {
        backend_config.backend
    } else {
        OcrBackend::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_ocr_backend_requires_policy_and_backend_opt_in() {
        let backend = OcrBackendConfig {
            backend: OcrBackend::Ocrmypdf,
            ..OcrBackendConfig::default()
        };

        assert_eq!(
            effective_ocr_backend(OcrFallbackPolicy::off(), &backend),
            OcrBackend::None
        );
        assert_eq!(
            effective_ocr_backend(
                OcrFallbackPolicy::on_quality_warning(),
                &OcrBackendConfig::default()
            ),
            OcrBackend::None
        );
        assert_eq!(
            effective_ocr_backend(OcrFallbackPolicy::on_quality_warning(), &backend),
            OcrBackend::Ocrmypdf
        );
    }

    #[test]
    fn effective_ocr_backend_keeps_unimplemented_tesseract_disabled() {
        let backend = OcrBackendConfig {
            backend: OcrBackend::Tesseract,
            ..OcrBackendConfig::default()
        };

        assert_eq!(
            effective_ocr_backend(OcrFallbackPolicy::on_quality_warning(), &backend),
            OcrBackend::None
        );
    }
}
