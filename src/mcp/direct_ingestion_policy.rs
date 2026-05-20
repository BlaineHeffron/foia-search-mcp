#[cfg(test)]
use std::fmt;

use reqwest::Url;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrustedDirectIngestionPolicy {
    entries: Vec<TrustedDirectIngestionEntry>,
}

impl TrustedDirectIngestionPolicy {
    pub(crate) fn deny_all() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_fixture_text(fixture_text: &str) -> Result<Self, TrustedPolicyError> {
        let mut entries = Vec::new();
        for (index, raw_line) in fixture_text.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((source, prefix)) = line.split_once(char::is_whitespace) else {
                return Err(TrustedPolicyError::MalformedLine {
                    line: index + 1,
                    content: line.to_owned(),
                });
            };
            let prefix = prefix.trim();
            if prefix.is_empty() {
                return Err(TrustedPolicyError::MalformedLine {
                    line: index + 1,
                    content: line.to_owned(),
                });
            }
            entries.push(TrustedDirectIngestionEntry::new(source.trim(), prefix)?);
        }
        Ok(Self { entries })
    }

    pub(crate) fn allows_source_id_url(&self, source: &str, source_id: &str) -> bool {
        let Ok(candidate) = Url::parse(source_id.trim()) else {
            return false;
        };
        if candidate.scheme() != "https" {
            return false;
        }
        if !candidate.username().is_empty() || candidate.password().is_some() {
            return false;
        }
        self.entries
            .iter()
            .filter(|entry| entry.source == source)
            .any(|entry| entry.matches(&candidate))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrustedDirectIngestionEntry {
    source: String,
    prefix: Url,
}

impl TrustedDirectIngestionEntry {
    #[cfg(test)]
    fn new(source: &str, prefix: &str) -> Result<Self, TrustedPolicyError> {
        if source.is_empty() {
            return Err(TrustedPolicyError::MalformedSource);
        }
        let prefix = Url::parse(prefix).map_err(|_| TrustedPolicyError::MalformedPrefix)?;
        if prefix.scheme() != "https" {
            return Err(TrustedPolicyError::UnsupportedScheme(
                prefix.scheme().to_owned(),
            ));
        }
        if !prefix.username().is_empty() || prefix.password().is_some() {
            return Err(TrustedPolicyError::CredentialsNotAllowed);
        }
        if prefix.host_str().is_none() {
            return Err(TrustedPolicyError::MalformedPrefix);
        }
        if prefix.path() == "/" {
            return Err(TrustedPolicyError::RootPathNotAllowed);
        }
        Ok(Self {
            source: source.to_owned(),
            prefix,
        })
    }

    fn matches(&self, candidate: &Url) -> bool {
        self.prefix.scheme() == candidate.scheme()
            && same_host_and_port(&self.prefix, candidate)
            && path_matches_prefix_boundary(self.prefix.path(), candidate.path())
    }
}

fn same_host_and_port(prefix: &Url, candidate: &Url) -> bool {
    let Some(prefix_host) = prefix.host_str() else {
        return false;
    };
    let Some(candidate_host) = candidate.host_str() else {
        return false;
    };
    if !prefix_host.eq_ignore_ascii_case(candidate_host) {
        return false;
    }
    prefix.port_or_known_default() == candidate.port_or_known_default()
}

fn path_matches_prefix_boundary(prefix_path: &str, candidate_path: &str) -> bool {
    if prefix_path == "/" || candidate_path == prefix_path {
        return true;
    }
    let required_prefix = if prefix_path.ends_with('/') {
        prefix_path.to_owned()
    } else {
        format!("{prefix_path}/")
    };
    candidate_path.starts_with(&required_prefix)
}

#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum TrustedPolicyError {
    MalformedLine { line: usize, content: String },
    MalformedSource,
    MalformedPrefix,
    RootPathNotAllowed,
    UnsupportedScheme(String),
    CredentialsNotAllowed,
}

#[cfg(test)]
impl fmt::Display for TrustedPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedLine { line, content } => {
                write!(f, "line {line} is invalid: {content:?}")
            }
            Self::MalformedSource => write!(f, "source name must not be empty"),
            Self::MalformedPrefix => write!(f, "trusted direct-ingestion prefix must be a URL"),
            Self::RootPathNotAllowed => write!(
                f,
                "trusted direct-ingestion prefix must include at least one path segment"
            ),
            Self::UnsupportedScheme(scheme) => write!(
                f,
                "trusted direct-ingestion prefix scheme must be https; got {scheme:?}"
            ),
            Self::CredentialsNotAllowed => {
                write!(
                    f,
                    "trusted direct-ingestion prefix must not include credentials"
                )
            }
        }
    }
}

#[cfg(test)]
impl std::error::Error for TrustedPolicyError {}

#[cfg(test)]
mod tests {
    use super::{TrustedDirectIngestionPolicy, TrustedPolicyError};

    #[test]
    fn deny_all_never_allows_url_source_ids() {
        let policy = TrustedDirectIngestionPolicy::deny_all();

        assert!(!policy.allows_source_id_url(
            "cia",
            "https://www.cia.gov/readingroom/document/cia-rdp-example"
        ));
    }

    #[test]
    fn fixture_policy_allows_only_configured_source_scheme_host_port_and_path_prefix() {
        let fixture =
            include_str!("../../tests/fixtures/ingest/trusted_direct_ingestion_allowlist.txt");
        let policy = TrustedDirectIngestionPolicy::from_fixture_text(fixture)
            .expect("fixture policy should parse");

        assert!(policy.allows_source_id_url(
            "cia",
            "https://www.cia.gov/readingroom/document/cia-rdp-example"
        ));
        assert!(policy.allows_source_id_url("nara", "https://catalog.archives.gov/id/123456"));
        assert!(!policy.allows_source_id_url(
            "cia",
            "https://www.cia.gov/readingroom/collection/cia-rdp-example"
        ));
        assert!(!policy.allows_source_id_url(
            "cia",
            "http://www.cia.gov/readingroom/document/cia-rdp-example"
        ));
        assert!(!policy.allows_source_id_url(
            "cia",
            "https://evil.example/readingroom/document/cia-rdp-example"
        ));
        assert!(
            !policy.allows_source_id_url("nara", "https://catalog.archives.gov/description/123456")
        );
        assert!(!policy.allows_source_id_url(
            "cia",
            "https://www.cia.gov/readingroom/documentary/cia-rdp-example"
        ));
    }

    #[test]
    fn fixture_policy_rejects_malformed_entries() {
        let err = TrustedDirectIngestionPolicy::from_fixture_text("cia")
            .expect_err("missing prefix should fail");
        assert!(matches!(err, TrustedPolicyError::MalformedLine { .. }));

        let err = TrustedDirectIngestionPolicy::from_fixture_text("cia ftp://example.test/docs/")
            .expect_err("unsupported scheme should fail");
        assert!(matches!(err, TrustedPolicyError::UnsupportedScheme(_)));

        let err = TrustedDirectIngestionPolicy::from_fixture_text("cia https://www.cia.gov/")
            .expect_err("root path prefix should fail");
        assert!(matches!(err, TrustedPolicyError::RootPathNotAllowed));
    }
}
