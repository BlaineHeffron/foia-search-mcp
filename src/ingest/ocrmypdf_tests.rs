use crate::ingest::{
    OcrmypdfConfig, OcrmypdfExtractor, PdftotextConfig, TextExtraction, TextExtractor,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
fn write_executable(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).expect("write fake binary");
    let mut permissions = fs::metadata(&path)
        .expect("fake binary metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("make fake binary executable");
    path
}

#[cfg(unix)]
fn input_pdf(dir: &Path) -> PathBuf {
    let path = dir.join("input.pdf");
    fs::write(&path, "%PDF-1.7\n").expect("write input pdf placeholder");
    path
}

#[cfg(unix)]
#[test]
fn ocrmypdf_output_is_reparsed_with_pdftotext() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let ocrmypdf = write_executable(
        tempdir.path(),
        "fake-ocrmypdf",
        r#"#!/bin/sh
out=""
for arg in "$@"; do out="$arg"; done
printf '%%PDF-1.7\nOCR PDF bytes' > "$out"
"#,
    );
    let pdftotext = write_executable(
        tempdir.path(),
        "fake-pdftotext",
        r#"#!/bin/sh
out=""
for arg in "$@"; do out="$arg"; done
printf 'OCR one\n\fOCR two\n' > "$out"
"#,
    );
    let mut config = OcrmypdfConfig::new(ocrmypdf, Duration::from_secs(1), 1024);
    config.pdftotext = PdftotextConfig::new(pdftotext);
    let extractor = OcrmypdfExtractor::new(config);

    let extracted = extractor
        .extract_pages(&input_pdf(tempdir.path()))
        .expect("extract OCR text");

    assert_eq!(extracted.pages.len(), 2);
    assert_eq!(extracted.pages[0].text, "OCR one");
    assert_eq!(extracted.pages[1].page_number, 2);
}

#[cfg(unix)]
#[test]
fn invalid_ocrmypdf_output_is_rejected_before_reparse() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let ocrmypdf = write_executable(
        tempdir.path(),
        "fake-ocrmypdf",
        r#"#!/bin/sh
out=""
for arg in "$@"; do out="$arg"; done
printf 'not a pdf' > "$out"
"#,
    );
    let pdftotext = write_executable(
        tempdir.path(),
        "fake-pdftotext",
        r#"#!/bin/sh
exit 0
"#,
    );
    let mut config = OcrmypdfConfig::new(ocrmypdf, Duration::from_secs(1), 1024);
    config.pdftotext = PdftotextConfig::new(pdftotext);
    let extractor = OcrmypdfExtractor::new(config);

    let error = extractor
        .extract_pages(&input_pdf(tempdir.path()))
        .expect_err("invalid OCR output should fail");

    assert!(matches!(
        error,
        TextExtraction::InvalidOutputPath { reason, .. } if reason == "OCR output is not a PDF"
    ));
}

#[cfg(unix)]
#[test]
fn missing_ocrmypdf_binary_returns_unavailable() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let missing = tempdir.path().join("missing-ocrmypdf");
    let config = OcrmypdfConfig::new(&missing, Duration::from_secs(1), 1024);
    let extractor = OcrmypdfExtractor::new(config);

    let error = extractor
        .extract_pages(&input_pdf(tempdir.path()))
        .expect_err("missing binary should fail");

    assert!(matches!(
        error,
        TextExtraction::UnavailableBinary { binary } if binary == missing
    ));
}

#[cfg(unix)]
#[test]
fn nonzero_ocrmypdf_exit_captures_bounded_stderr() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let ocrmypdf = write_executable(
        tempdir.path(),
        "fake-ocrmypdf",
        r#"#!/bin/sh
printf 'abcdefghijklmnopqrstuvwxyz' >&2
exit 9
"#,
    );
    let config = OcrmypdfConfig::new(ocrmypdf, Duration::from_secs(1), 10);
    let extractor = OcrmypdfExtractor::new(config);

    let error = extractor
        .extract_pages(&input_pdf(tempdir.path()))
        .expect_err("nonzero exit should fail");

    match error {
        TextExtraction::CommandFailed { status, stderr, .. } => {
            assert!(status.contains('9'));
            assert_eq!(stderr, "abcdefghij... [truncated]");
        }
        other => panic!("expected command failure, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn ocrmypdf_timeout_kills_process() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let ocrmypdf = write_executable(
        tempdir.path(),
        "fake-ocrmypdf",
        r#"#!/bin/sh
printf 'started' >&2
sleep 5
"#,
    );
    let config = OcrmypdfConfig::new(ocrmypdf.clone(), Duration::from_millis(250), 1024);
    let extractor = OcrmypdfExtractor::new(config);

    let error = extractor
        .extract_pages(&input_pdf(tempdir.path()))
        .expect_err("timeout should fail");

    assert!(matches!(
        error,
        TextExtraction::Timeout { binary, .. } if binary == ocrmypdf
    ));
}

#[cfg(unix)]
#[test]
fn ocrmypdf_cancellation_kills_process() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let ocrmypdf = write_executable(
        tempdir.path(),
        "fake-ocrmypdf",
        r#"#!/bin/sh
printf 'started' >&2
sleep 5
"#,
    );
    let config = OcrmypdfConfig::new(ocrmypdf.clone(), Duration::from_secs(1), 1024);
    let extractor = OcrmypdfExtractor::new(config);

    let error = extractor
        .extract_pages_with_cancel(&input_pdf(tempdir.path()), &|| true)
        .expect_err("cancellation should fail");

    assert!(matches!(
        error,
        TextExtraction::Cancelled { binary, .. } if binary == ocrmypdf
    ));
}
