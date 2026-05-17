use crate::ingest::pdf::{ExtractedText, TextExtraction, TextExtractor};
use crate::ingest::{PdftotextConfig, PdftotextExtractor};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::raw::c_int;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(unix)]
const SIGKILL: c_int = 9;
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const OCR_OUTPUT_FILENAME: &str = "ocr-output.pdf";
const STDERR_FILENAME: &str = "stderr.txt";
const PDF_HEADER_PREFIX: &[u8] = b"%PDF-";

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: c_int, sig: c_int) -> c_int;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OcrmypdfConfig {
    pub binary: PathBuf,
    pub timeout: Duration,
    pub args: Vec<OsString>,
    pub max_stderr_bytes: usize,
    pub pdftotext: PdftotextConfig,
}

impl OcrmypdfConfig {
    pub fn new(binary: impl Into<PathBuf>, timeout: Duration, max_stderr_bytes: usize) -> Self {
        Self {
            binary: binary.into(),
            timeout,
            max_stderr_bytes,
            ..Self::default()
        }
    }
}

impl Default for OcrmypdfConfig {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("ocrmypdf"),
            timeout: Duration::from_secs(300),
            args: vec![
                OsString::from("--skip-text"),
                OsString::from("--output-type"),
                OsString::from("pdf"),
            ],
            max_stderr_bytes: 8 * 1024,
            pdftotext: PdftotextConfig::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct OcrmypdfExtractor {
    config: OcrmypdfConfig,
}

impl OcrmypdfExtractor {
    pub fn new(config: OcrmypdfConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &OcrmypdfConfig {
        &self.config
    }
}

impl TextExtractor for OcrmypdfExtractor {
    fn extract_pages(&self, path: &Path) -> Result<ExtractedText, TextExtraction> {
        let temp_dir = OcrTempDir::create()?;
        let output_path = temp_dir.path().join(OCR_OUTPUT_FILENAME);
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
            .arg(path)
            .arg(&output_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_file.try_clone()?));
        #[cfg(unix)]
        command.process_group(0);

        let status = match command.spawn() {
            Ok(mut child) => wait_for_child(&mut child, &mut stderr_file, &self.config)?,
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

        validate_pdf_output(&output_path)?;
        PdftotextExtractor::new(self.config.pdftotext.clone()).extract_pages(&output_path)
    }
}

fn wait_for_child(
    child: &mut Child,
    stderr_file: &mut File,
    config: &OcrmypdfConfig,
) -> Result<std::process::ExitStatus, TextExtraction> {
    let deadline = Instant::now() + config.timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            kill_child_process_tree(child);
            let _ = child.wait();
            return Err(TextExtraction::Timeout {
                binary: config.binary.clone(),
                timeout: config.timeout,
                stderr: read_bounded_file(stderr_file, config.max_stderr_bytes)?,
            });
        }
        std::thread::sleep(WAIT_POLL_INTERVAL);
    }
}

fn validate_pdf_output(path: &Path) -> Result<(), TextExtraction> {
    if fs::metadata(path)?.len() == 0 {
        return Err(TextExtraction::EmptyInput);
    }

    let mut file = File::open(path)?;
    let mut header = [0_u8; 5];
    file.read_exact(&mut header)?;
    if header != PDF_HEADER_PREFIX {
        return Err(TextExtraction::InvalidOutputPath {
            path: path.to_path_buf(),
            reason: "OCR output is not a PDF".to_owned(),
        });
    }

    Ok(())
}

fn kill_child_process_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Ok(pid) = c_int::try_from(child.id()) {
            // SAFETY: the child was started in its own process group with
            // process_group(0), so SIGKILL to -pid is scoped to this command.
            let killed_group = unsafe { kill(-pid, SIGKILL) } == 0;
            if killed_group {
                return;
            }
        }
    }

    let _ = child.kill();
}

struct OcrTempDir {
    path: PathBuf,
}

impl OcrTempDir {
    fn create() -> io::Result<Self> {
        let base = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        for attempt in 0..100 {
            let path = base.join(format!(
                "foia-search-ocrmypdf-{}-{nonce}-{attempt}",
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
            "could not create unique ocrmypdf temp directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for OcrTempDir {
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
                "parent {} does not match OCR temp directory {}",
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
