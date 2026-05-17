use crate::ingest::pdf::{
    extracted_text_from_form_feed, ExtractedText, TextExtraction, TextExtractor,
};
use crate::ingest::process::{wait_for_child_with_controls, ChildWaitOutcome};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_MAX_STDERR_BYTES: usize = 8 * 1024;
const OUTPUT_FILENAME: &str = "extracted.txt";
const STDERR_FILENAME: &str = "stderr.txt";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PdftotextConfig {
    pub binary: PathBuf,
    pub timeout: Duration,
    pub args: Vec<OsString>,
    pub max_stderr_bytes: usize,
}

impl PdftotextConfig {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            ..Self::default()
        }
    }
}

impl Default for PdftotextConfig {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("pdftotext"),
            timeout: DEFAULT_TIMEOUT,
            args: vec![
                OsString::from("-layout"),
                OsString::from("-enc"),
                OsString::from("UTF-8"),
            ],
            max_stderr_bytes: DEFAULT_MAX_STDERR_BYTES,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PdftotextExtractor {
    config: PdftotextConfig,
}

impl PdftotextExtractor {
    pub fn new(config: PdftotextConfig) -> Self {
        Self { config }
    }

    pub fn with_binary(binary: impl Into<PathBuf>) -> Self {
        Self::new(PdftotextConfig::new(binary))
    }

    pub fn config(&self) -> &PdftotextConfig {
        &self.config
    }
}

impl Default for PdftotextExtractor {
    fn default() -> Self {
        Self::new(PdftotextConfig::default())
    }
}

impl TextExtractor for PdftotextExtractor {
    fn extract_pages(&self, path: &Path) -> Result<ExtractedText, TextExtraction> {
        self.extract_pdf_text(path, &|| false)
    }

    fn extract_pages_with_cancel(
        &self,
        path: &Path,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ExtractedText, TextExtraction> {
        self.extract_pdf_text(path, is_cancelled)
    }
}

impl PdftotextExtractor {
    fn extract_pdf_text(
        &self,
        input_path: &Path,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ExtractedText, TextExtraction> {
        let temp_dir = ExtractionTempDir::create()?;
        let output_path = temp_dir.path().join(OUTPUT_FILENAME);
        let stderr_path = temp_dir.path().join(STDERR_FILENAME);
        validate_child_output_path(temp_dir.path(), &output_path)?;
        validate_child_output_path(temp_dir.path(), &stderr_path)?;

        let mut stderr_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&stderr_path)?;
        let mut command = Command::new(&self.config.binary);
        command
            .args(&self.config.args)
            .arg(input_path)
            .arg(&output_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_file.try_clone()?));
        #[cfg(unix)]
        command.process_group(0);

        let status = match command.spawn() {
            Ok(mut child) => {
                match wait_for_child_with_controls(&mut child, self.config.timeout, is_cancelled)? {
                    ChildWaitOutcome::Completed(status) => status,
                    ChildWaitOutcome::TimedOut => {
                        return Err(TextExtraction::Timeout {
                            binary: self.config.binary.clone(),
                            timeout: self.config.timeout,
                            stderr: read_bounded_file(
                                &mut stderr_file,
                                self.config.max_stderr_bytes,
                            )?,
                        });
                    }
                    ChildWaitOutcome::Cancelled => {
                        return Err(TextExtraction::Cancelled {
                            binary: self.config.binary.clone(),
                            stderr: read_bounded_file(
                                &mut stderr_file,
                                self.config.max_stderr_bytes,
                            )?,
                        });
                    }
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Err(TextExtraction::UnavailableBinary {
                    binary: self.config.binary.clone(),
                });
            }
            Err(err) => return Err(TextExtraction::Io(err)),
        };

        if !status.success() {
            return Err(TextExtraction::CommandFailed {
                binary: self.config.binary.clone(),
                status: status.to_string(),
                stderr: read_bounded_file(&mut stderr_file, self.config.max_stderr_bytes)?,
            });
        }

        let text = fs::read_to_string(&output_path)?;
        extracted_text_from_form_feed(&text)
    }
}

struct ExtractionTempDir {
    path: PathBuf,
}

impl ExtractionTempDir {
    fn create() -> io::Result<Self> {
        let base = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        for attempt in 0..100 {
            let path = base.join(format!(
                "foia-search-pdftotext-{}-{nonce}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create unique pdftotext temp directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ExtractionTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn validate_child_output_path(parent: &Path, child: &Path) -> Result<(), TextExtraction> {
    let canonical_parent = parent.canonicalize()?;
    let child_parent = child
        .parent()
        .ok_or_else(|| TextExtraction::InvalidOutputPath {
            path: child.to_path_buf(),
            reason: "missing parent directory".to_owned(),
        })?;
    let canonical_child_parent = child_parent.canonicalize()?;

    if canonical_parent != canonical_child_parent {
        return Err(TextExtraction::InvalidOutputPath {
            path: child.to_path_buf(),
            reason: format!(
                "parent {} does not match extraction temp directory {}",
                canonical_child_parent.display(),
                canonical_parent.display()
            ),
        });
    }

    if child.file_name().is_none() {
        return Err(TextExtraction::InvalidOutputPath {
            path: child.to_path_buf(),
            reason: "missing file name".to_owned(),
        });
    }

    Ok(())
}

fn read_bounded_file(file: &mut File, max_bytes: usize) -> io::Result<String> {
    file.flush()?;
    file.seek(SeekFrom::Start(0))?;
    let file_len = file.metadata()?.len();
    let mut buffer = Vec::new();
    file.take(max_bytes as u64).read_to_end(&mut buffer)?;
    let mut text = String::from_utf8_lossy(&buffer).into_owned();
    if file_len > max_bytes as u64 {
        text.push_str("... [truncated]");
    }
    Ok(text.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    fn write_executable(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        {
            let mut file = File::create(&path).expect("create fake binary");
            file.write_all(body.as_bytes()).expect("write fake binary");
            file.sync_all().expect("sync fake binary");
        }
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
    fn extracts_form_feed_pages_from_external_output() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        let binary = write_executable(
            tempdir.path(),
            "fake-pdftotext",
            r#"#!/bin/sh
out=""
for arg in "$@"; do out="$arg"; done
printf 'First page\n\fSecond page\n' > "$out"
"#,
        );
        let extractor = PdftotextExtractor::with_binary(binary);

        let extracted = extractor
            .extract_pages(&input_pdf(tempdir.path()))
            .expect("extract pages");

        assert_eq!(extracted.pages.len(), 2);
        assert_eq!(extracted.pages[0].page_number, 1);
        assert_eq!(extracted.pages[0].text, "First page");
        assert_eq!(extracted.pages[1].page_number, 2);
        assert_eq!(extracted.pages[1].text, "Second page");
    }

    #[cfg(unix)]
    #[test]
    fn missing_binary_returns_explicit_error() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        let missing_binary = tempdir.path().join("missing-pdftotext");
        let extractor = PdftotextExtractor::with_binary(&missing_binary);

        let error = extractor
            .extract_pages(&input_pdf(tempdir.path()))
            .expect_err("missing binary should fail");

        assert!(matches!(
            error,
            TextExtraction::UnavailableBinary { binary } if binary == missing_binary
        ));
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_exit_captures_bounded_stderr() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        let binary = write_executable(
            tempdir.path(),
            "fake-pdftotext",
            r#"#!/bin/sh
printf 'abcdefghijklmnopqrstuvwxyz' >&2
exit 7
"#,
        );
        let mut config = PdftotextConfig::new(binary);
        config.max_stderr_bytes = 10;
        let extractor = PdftotextExtractor::new(config);

        let error = extractor
            .extract_pages(&input_pdf(tempdir.path()))
            .expect_err("nonzero exit should fail");

        match error {
            TextExtraction::CommandFailed { status, stderr, .. } => {
                assert!(status.contains('7'));
                assert_eq!(stderr, "abcdefghij... [truncated]");
            }
            other => panic!("expected command failure, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_process_and_returns_timeout_error() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        let binary = write_executable(
            tempdir.path(),
            "fake-pdftotext",
            r#"#!/bin/sh
printf 'started' >&2
sleep 5
"#,
        );
        let mut config = PdftotextConfig::new(binary.clone());
        config.timeout = Duration::from_millis(25);
        let extractor = PdftotextExtractor::new(config);

        let error = extractor
            .extract_pages(&input_pdf(tempdir.path()))
            .expect_err("timeout should fail");

        assert!(matches!(
            error,
            TextExtraction::Timeout { binary: err_binary, .. } if err_binary == binary
        ));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_kills_process_and_returns_cancelled_error() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        let binary = write_executable(
            tempdir.path(),
            "fake-pdftotext",
            r#"#!/bin/sh
printf 'started' >&2
sleep 5
"#,
        );
        let mut config = PdftotextConfig::new(binary.clone());
        config.timeout = Duration::from_secs(1);
        let extractor = PdftotextExtractor::new(config);

        let error = extractor
            .extract_pages_with_cancel(&input_pdf(tempdir.path()), &|| true)
            .expect_err("cancellation should fail");

        assert!(matches!(
            error,
            TextExtraction::Cancelled { binary: err_binary, .. } if err_binary == binary
        ));
    }

    #[cfg(unix)]
    #[test]
    fn blank_pages_are_preserved_and_warned_for() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        let binary = write_executable(
            tempdir.path(),
            "fake-pdftotext",
            r#"#!/bin/sh
out=""
for arg in "$@"; do out="$arg"; done
printf 'First physical page\n\f   \n\fThird physical page\n' > "$out"
"#,
        );
        let extractor = PdftotextExtractor::with_binary(binary);

        let extracted = extractor
            .extract_pages(&input_pdf(tempdir.path()))
            .expect("extract pages with blank middle page");

        assert_eq!(extracted.pages.len(), 3);
        assert_eq!(extracted.pages[1].page_number, 2);
        assert!(extracted.pages[1].text.is_empty());
        assert!(extracted
            .warnings
            .iter()
            .any(|warning| warning.contains("blank page")));
    }
}
