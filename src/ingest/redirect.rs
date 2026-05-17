use crate::store::CacheEntry;
use reqwest::header::{IF_MODIFIED_SINCE, IF_NONE_MATCH, LOCATION, USER_AGENT};
use reqwest::Url;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedirectPolicy {
    Deny,
    Follow(RedirectFollowPolicy),
}

impl RedirectPolicy {
    pub fn same_host(max_hops: usize) -> Self {
        Self::Follow(RedirectFollowPolicy {
            max_hops,
            allow_cross_host: false,
        })
    }

    pub fn allow_cross_host(max_hops: usize) -> Self {
        Self::Follow(RedirectFollowPolicy {
            max_hops,
            allow_cross_host: true,
        })
    }
}

impl Default for RedirectPolicy {
    fn default() -> Self {
        Self::Deny
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RedirectFollowPolicy {
    pub max_hops: usize,
    pub allow_cross_host: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedirectValidationError {
    Denied,
    MalformedLocation(String),
    UnsupportedScheme(String),
    CredentialsNotAllowed,
    CrossHostDenied {
        original_host: String,
        redirect_host: String,
    },
    UnsafeHost(String),
}

pub type RedirectValidationResult<T> = Result<T, RedirectValidationError>;

#[derive(Debug)]
pub enum RedirectFollowError {
    Request(reqwest::Error),
    Denied {
        url: String,
        location: Option<String>,
        message: String,
    },
    Unsafe {
        url: String,
        location: String,
        message: String,
    },
    TooMany {
        url: String,
        max_hops: usize,
    },
}

pub async fn send_with_redirects(
    client: &reqwest::Client,
    initial_url: &Url,
    cached: Option<&CacheEntry>,
    redirect_policy: RedirectPolicy,
    user_agent: &str,
) -> Result<(reqwest::Response, Url), RedirectFollowError> {
    let mut current_url = initial_url.clone();
    let max_hops = match redirect_policy {
        RedirectPolicy::Deny => 0,
        RedirectPolicy::Follow(policy) => policy.max_hops,
    };

    for hop_count in 0..=max_hops {
        let mut builder = client
            .get(current_url.clone())
            .header(USER_AGENT, user_agent);
        if hop_count == 0 {
            if let Some(entry) = cached {
                if let Some(etag) = entry.etag.as_deref() {
                    builder = builder.header(IF_NONE_MATCH, etag);
                }
                if let Some(last_modified) = entry.last_modified.as_deref() {
                    builder = builder.header(IF_MODIFIED_SINCE, last_modified);
                }
            }
        }

        let response = builder.send().await.map_err(RedirectFollowError::Request)?;
        if !is_follow_redirect_status(response.status()) {
            return Ok((response, current_url));
        }

        let location = header_string(response.headers(), LOCATION);
        if matches!(redirect_policy, RedirectPolicy::Deny) {
            return Err(RedirectFollowError::Denied {
                url: current_url.to_string(),
                location,
                message: "source redirect policy is deny".to_owned(),
            });
        }
        let location = location.ok_or_else(|| RedirectFollowError::Denied {
            url: current_url.to_string(),
            location: None,
            message: "redirect response did not include a Location header".to_owned(),
        })?;
        if hop_count >= max_hops {
            return Err(RedirectFollowError::TooMany {
                url: current_url.to_string(),
                max_hops,
            });
        }
        current_url =
            validate_redirect_location(redirect_policy, initial_url, &current_url, &location)
                .map_err(|err| redirect_validation_error(current_url.as_str(), &location, err))?;
    }

    Err(RedirectFollowError::TooMany {
        url: current_url.to_string(),
        max_hops,
    })
}

pub fn validate_redirect_location(
    policy: RedirectPolicy,
    original_url: &Url,
    current_url: &Url,
    location: &str,
) -> RedirectValidationResult<Url> {
    let follow_policy = match policy {
        RedirectPolicy::Deny => return Err(RedirectValidationError::Denied),
        RedirectPolicy::Follow(follow_policy) => follow_policy,
    };

    let next = current_url
        .join(location)
        .map_err(|err| RedirectValidationError::MalformedLocation(err.to_string()))?;
    validate_http_url(&next)?;
    validate_no_credentials(&next)?;
    if !same_host(original_url, &next) {
        validate_safe_host(&next)?;
    }

    if !follow_policy.allow_cross_host && !same_host(original_url, &next) {
        return Err(RedirectValidationError::CrossHostDenied {
            original_host: host_label(original_url),
            redirect_host: host_label(&next),
        });
    }

    Ok(next)
}

fn validate_http_url(url: &Url) -> RedirectValidationResult<()> {
    match url.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(RedirectValidationError::UnsupportedScheme(
            scheme.to_owned(),
        )),
    }
}

fn redirect_validation_error(
    url: &str,
    location: &str,
    err: RedirectValidationError,
) -> RedirectFollowError {
    match err {
        RedirectValidationError::Denied => RedirectFollowError::Denied {
            url: url.to_owned(),
            location: Some(location.to_owned()),
            message: "source redirect policy is deny".to_owned(),
        },
        RedirectValidationError::MalformedLocation(message) => RedirectFollowError::Unsafe {
            url: url.to_owned(),
            location: location.to_owned(),
            message: format!("malformed Location header: {message}"),
        },
        RedirectValidationError::UnsupportedScheme(scheme) => RedirectFollowError::Unsafe {
            url: url.to_owned(),
            location: location.to_owned(),
            message: format!(
                "unsupported redirect scheme {scheme}; only http and https are allowed"
            ),
        },
        RedirectValidationError::CredentialsNotAllowed => RedirectFollowError::Unsafe {
            url: url.to_owned(),
            location: location.to_owned(),
            message: "redirect target includes credentials".to_owned(),
        },
        RedirectValidationError::CrossHostDenied {
            original_host,
            redirect_host,
        } => RedirectFollowError::Unsafe {
            url: url.to_owned(),
            location: location.to_owned(),
            message: format!(
                "cross-host redirect from {original_host} to {redirect_host} is not allowed by policy"
            ),
        },
        RedirectValidationError::UnsafeHost(host) => RedirectFollowError::Unsafe {
            url: url.to_owned(),
            location: location.to_owned(),
            message: format!("redirect target host {host} is blocked"),
        },
    }
}

fn is_follow_redirect_status(status: reqwest::StatusCode) -> bool {
    matches!(
        status,
        reqwest::StatusCode::MOVED_PERMANENTLY
            | reqwest::StatusCode::FOUND
            | reqwest::StatusCode::SEE_OTHER
            | reqwest::StatusCode::TEMPORARY_REDIRECT
            | reqwest::StatusCode::PERMANENT_REDIRECT
    )
}

fn header_string(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn validate_no_credentials(url: &Url) -> RedirectValidationResult<()> {
    if url.username().is_empty() && url.password().is_none() {
        Ok(())
    } else {
        Err(RedirectValidationError::CredentialsNotAllowed)
    }
}

fn validate_safe_host(url: &Url) -> RedirectValidationResult<()> {
    let host = url
        .host_str()
        .ok_or_else(|| RedirectValidationError::UnsafeHost("missing host".to_owned()))?;
    if is_blocked_hostname(host) {
        return Err(RedirectValidationError::UnsafeHost(host.to_owned()));
    }
    let ip_host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = ip_host.parse::<IpAddr>() {
        if is_unsafe_ip(ip) {
            return Err(RedirectValidationError::UnsafeHost(host.to_owned()));
        }
    }
    Ok(())
}

fn is_blocked_hostname(host: &str) -> bool {
    let normalized = host.trim_end_matches('.').to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "localhost" | "localhost.localdomain" | "metadata.google.internal"
    )
}

fn same_host(left: &Url, right: &Url) -> bool {
    host_label(left).eq_ignore_ascii_case(&host_label(right))
}

fn host_label(url: &Url) -> String {
    url.host_str()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_owned()
}

fn is_unsafe_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_unsafe_ipv4(ip),
        IpAddr::V6(ip) => is_unsafe_ipv6(ip),
    }
}

fn is_unsafe_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.octets()[0] == 0
        || ip == Ipv4Addr::new(169, 254, 169, 254)
}

fn is_unsafe_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || is_ipv6_unique_local(ip)
        || is_ipv6_unicast_link_local(ip)
}

fn is_ipv6_unique_local(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn is_ipv6_unicast_link_local(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_location() {
        let original = Url::parse("https://example.test/dir/source.pdf").expect("url");
        let next = validate_redirect_location(
            RedirectPolicy::same_host(3),
            &original,
            &original,
            "../final.pdf",
        )
        .expect("valid redirect");

        assert_eq!(next.as_str(), "https://example.test/final.pdf");
    }

    #[test]
    fn rejects_loopback_literal() {
        let original = Url::parse("https://example.test/asset.pdf").expect("url");
        let error = validate_redirect_location(
            RedirectPolicy::allow_cross_host(3),
            &original,
            &original,
            "http://127.0.0.1/private.pdf",
        )
        .expect_err("loopback should be unsafe");

        assert!(matches!(error, RedirectValidationError::UnsafeHost(_)));
    }

    #[test]
    fn rejects_malformed_location() {
        let original = Url::parse("https://example.test/asset.pdf").expect("url");
        let error = validate_redirect_location(
            RedirectPolicy::same_host(3),
            &original,
            &original,
            "http://[::1",
        )
        .expect_err("malformed Location should fail");

        assert!(matches!(
            error,
            RedirectValidationError::MalformedLocation(_)
        ));
    }

    #[test]
    fn rejects_unsafe_cross_host_literals() {
        let original = Url::parse("https://example.test/asset.pdf").expect("url");
        for location in [
            "http://10.0.0.8/private.pdf",
            "http://169.254.1.2/private.pdf",
            "http://224.0.0.1/private.pdf",
            "http://[fc00::1]/private.pdf",
            "http://[fe80::1]/private.pdf",
            "http://metadata.google.internal/private.pdf",
        ] {
            let error = validate_redirect_location(
                RedirectPolicy::allow_cross_host(3),
                &original,
                &original,
                location,
            )
            .expect_err("unsafe target should fail");

            assert!(matches!(error, RedirectValidationError::UnsafeHost(_)));
        }
    }
}
