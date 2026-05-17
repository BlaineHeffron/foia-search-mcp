use crate::ingest::ocr::{
    OcrFallbackPolicy, OCR_FALLBACK_INCOMPATIBLE_WARNING, OCR_FALLBACK_RESCUED_WARNING,
    OCR_FALLBACK_USED_WARNING,
};
use crate::ingest::pdf::{ExtractedText, TextExtraction, TextExtractor};
use crate::store::TextSource;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedPdfText {
    pub extracted: ExtractedText,
    pub text_source: TextSource,
}

pub fn select_pdf_text(
    path: &Path,
    embedded_extractor: &dyn TextExtractor,
    ocr_extractor: &dyn TextExtractor,
    policy: OcrFallbackPolicy,
) -> Result<SelectedPdfText, TextExtraction> {
    match embedded_extractor.extract_pages(path) {
        Ok(embedded) => select_after_embedded_success(path, embedded, ocr_extractor, policy),
        Err(embedded_error) => {
            if !policy.is_enabled() {
                return Err(embedded_error);
            }
            match ocr_extractor.extract_pages(path) {
                Ok(mut ocr) => {
                    ocr.warnings
                        .insert(0, OCR_FALLBACK_RESCUED_WARNING.to_owned());
                    Ok(SelectedPdfText {
                        extracted: ocr,
                        text_source: TextSource::LocalOcr,
                    })
                }
                Err(_) => Err(embedded_error),
            }
        }
    }
}

fn select_after_embedded_success(
    path: &Path,
    embedded: ExtractedText,
    ocr_extractor: &dyn TextExtractor,
    policy: OcrFallbackPolicy,
) -> Result<SelectedPdfText, TextExtraction> {
    if embedded.warnings.is_empty() || !policy.is_enabled() {
        return Ok(SelectedPdfText {
            extracted: embedded,
            text_source: TextSource::EmbeddedPdfText,
        });
    }

    match ocr_extractor.extract_pages(path) {
        Ok(mut ocr) if compatible_page_boundaries(&embedded, &ocr) => {
            let mut warnings = embedded.warnings.clone();
            warnings.push(OCR_FALLBACK_USED_WARNING.to_owned());
            warnings.append(&mut ocr.warnings);
            ocr.warnings = warnings;
            Ok(SelectedPdfText {
                extracted: ocr,
                text_source: TextSource::LocalOcr,
            })
        }
        Ok(_) => {
            let mut embedded = embedded;
            embedded
                .warnings
                .push(OCR_FALLBACK_INCOMPATIBLE_WARNING.to_owned());
            Ok(SelectedPdfText {
                extracted: embedded,
                text_source: TextSource::EmbeddedPdfText,
            })
        }
        Err(_) => Ok(SelectedPdfText {
            extracted: embedded,
            text_source: TextSource::EmbeddedPdfText,
        }),
    }
}

fn compatible_page_boundaries(embedded: &ExtractedText, ocr: &ExtractedText) -> bool {
    embedded.pages.len() == ocr.pages.len()
        && embedded
            .pages
            .iter()
            .zip(&ocr.pages)
            .all(|(embedded_page, ocr_page)| embedded_page.page_number == ocr_page.page_number)
}
