use std::path::PathBuf;

use serde::Serialize;

const DEFAULT_DATA_DIR_NAME: &str = ".foia-search";

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub nara_api_key: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir = std::env::var("FOIA_SEARCH_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir_or_current().join(DEFAULT_DATA_DIR_NAME));
        let nara_api_key = std::env::var("FOIA_SEARCH_NARA_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());

        Self {
            data_dir,
            nara_api_key,
        }
    }

    pub fn source_status(&self) -> Vec<SourceStatus> {
        vec![
            SourceStatus {
                name: "cia".to_string(),
                enabled: false,
                status: "planned".to_string(),
                note: "CIA Reading Room adapter is not implemented in this scaffold.".to_string(),
            },
            SourceStatus {
                name: "nara".to_string(),
                enabled: false,
                status: if self.nara_api_key.is_some() {
                    "configured"
                } else {
                    "missing_api_key"
                }
                .to_string(),
                note: if self.nara_api_key.is_some() {
                    "FOIA_SEARCH_NARA_API_KEY is set; adapter implementation is pending."
                } else {
                    "Set FOIA_SEARCH_NARA_API_KEY before enabling the NARA adapter."
                }
                .to_string(),
            },
            SourceStatus {
                name: "govinfo".to_string(),
                enabled: false,
                status: "planned".to_string(),
                note: "GovInfo adapter is planned after CIA and NARA.".to_string(),
            },
            SourceStatus {
                name: "frus".to_string(),
                enabled: false,
                status: "planned".to_string(),
                note: "FRUS adapter is planned for document-level citations.".to_string(),
            },
            SourceStatus {
                name: "dtic".to_string(),
                enabled: false,
                status: "planned".to_string(),
                note: "DTIC adapter is planned and will require fragility warnings.".to_string(),
            },
            SourceStatus {
                name: "noaa".to_string(),
                enabled: false,
                status: "planned".to_string(),
                note: "NOAA repository adapter is planned.".to_string(),
            },
        ]
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceStatus {
    pub name: String,
    pub enabled: bool,
    pub status: String,
    pub note: String,
}

fn home_dir_or_current() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_status_reports_nara_key_state() {
        let config = Config {
            data_dir: PathBuf::from("/tmp/foia-search-test"),
            nara_api_key: Some("key".to_string()),
        };

        let nara = config
            .source_status()
            .into_iter()
            .find(|source| source.name == "nara");

        assert!(matches!(
            nara.as_ref().map(|source| source.status.as_str()),
            Some("configured")
        ));
    }
}
