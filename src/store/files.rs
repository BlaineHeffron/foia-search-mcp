use crate::store::{DocumentKey, StoreError};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlobKind {
    Pdf,
    Html,
    Other,
}

impl BlobKind {
    fn dirname(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Html => "html",
            Self::Other => "other",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Html => "html",
            Self::Other => "blob",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredBlob {
    pub sha256: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug)]
pub enum FileStoreError {
    Io(io::Error),
    Store(StoreError),
}

impl fmt::Display for FileStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "file store I/O error: {err}"),
            Self::Store(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for FileStoreError {}

impl From<io::Error> for FileStoreError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<StoreError> for FileStoreError {
    fn from(err: StoreError) -> Self {
        Self::Store(err)
    }
}

pub type FileStoreResult<T> = Result<T, FileStoreError>;

#[derive(Clone, Debug)]
pub struct ContentAddressedStore {
    root: PathBuf,
}

impl ContentAddressedStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put_reader(&self, kind: BlobKind, mut reader: impl Read) -> FileStoreResult<StoredBlob> {
        let tmp_dir = self.root.join("tmp");
        fs::create_dir_all(&tmp_dir)?;
        let tmp_path = tmp_dir.join(unique_tmp_name());
        let mut tmp = File::create(&tmp_path)?;
        let mut hasher = Sha256State::new();
        let mut size_bytes = 0_u64;
        let mut buffer = [0_u8; 32 * 1024];

        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            tmp.write_all(&buffer[..read])?;
            size_bytes += read as u64;
        }
        tmp.sync_all()?;

        let sha256 = hasher.finish_hex();
        let final_path = self.blob_path(kind, &sha256);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if !final_path.exists() {
            fs::rename(&tmp_path, &final_path)?;
        } else {
            fs::remove_file(&tmp_path)?;
        }

        Ok(StoredBlob {
            sha256,
            path: final_path,
            size_bytes,
        })
    }

    pub fn blob_path(&self, kind: BlobKind, sha256: &str) -> PathBuf {
        self.root
            .join("blobs")
            .join(kind.dirname())
            .join("sha256")
            .join(format!("{sha256}.{}", kind.extension()))
    }

    pub fn derived_document_text_path(&self, document_key: &DocumentKey) -> PathBuf {
        self.root()
            .join("text")
            .join("documents")
            .join(format!("{}.txt", document_key.as_str()))
    }

    pub fn derived_page_text_path(&self, document_key: &DocumentKey, page_number: u32) -> PathBuf {
        self.root()
            .join("text")
            .join("pages")
            .join(document_key.as_str())
            .join(format!("{page_number}.txt"))
    }
}

fn unique_tmp_name() -> String {
    let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("blob-{}-{sequence}", std::process::id())
}

struct Sha256State(sha2::Sha256);

impl Sha256State {
    fn new() -> Self {
        use sha2::Digest;
        Self(sha2::Sha256::new())
    }

    fn update(&mut self, bytes: &[u8]) {
        use sha2::Digest;
        self.0.update(bytes);
    }

    fn finish_hex(self) -> String {
        use sha2::Digest;
        let digest = self.0.finalize();
        let mut out = String::with_capacity(64);
        for byte in digest {
            use std::fmt::Write as _;
            let _ = write!(&mut out, "{byte:02x}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Arc;

    #[test]
    fn concurrent_put_reader_uses_unique_temp_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(ContentAddressedStore::new(tmp.path()));
        let first = {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                store
                    .put_reader(BlobKind::Pdf, Cursor::new(vec![b'a'; 64 * 1024]))
                    .expect("first blob")
            })
        };
        let second = {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                store
                    .put_reader(BlobKind::Pdf, Cursor::new(vec![b'b'; 64 * 1024]))
                    .expect("second blob")
            })
        };

        let first = first.join().expect("first thread");
        let second = second.join().expect("second thread");

        assert_ne!(first.sha256, second.sha256);
        assert!(first.path.exists());
        assert!(second.path.exists());
    }
}
