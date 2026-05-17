pub(crate) fn absolutize(href: &str, base_url: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_owned();
    }

    let base = base_url.trim_end_matches('/');
    if href.starts_with("//") {
        let scheme = base.split("://").next().unwrap_or("https");
        return format!("{scheme}:{href}");
    }
    if href.starts_with('/') {
        let origin = origin(base).unwrap_or_else(|| base.to_owned());
        return format!("{origin}{href}");
    }
    format!("{base}/{href}")
}

pub(crate) fn canonicalize_official_url(url: &str, base_url: &str) -> String {
    let Some(url_host_name) = url_host(url) else {
        return url.to_owned();
    };
    let Some(base_host) = url_host(base_url) else {
        return url.to_owned();
    };

    if url_host_name.eq_ignore_ascii_case("www.aaro.mil")
        && base_host.eq_ignore_ascii_case("www.aaro.mil")
        && url.starts_with("http://")
    {
        format!("https://{}", &url["http://".len()..])
    } else {
        url.to_owned()
    }
}

pub(crate) fn is_allowed_aaro_url(url: &str, base_url: &str) -> bool {
    let Some(url_origin) = normalized_origin(url) else {
        return false;
    };
    let Some(base_origin) = normalized_origin(base_url) else {
        return false;
    };
    if url_origin != base_origin {
        return false;
    }

    let path = url_path(url);
    !path.trim_matches('/').is_empty()
}

pub(crate) fn is_allowed_partner_asset_url(url: &str) -> bool {
    let Some(host) = url_host(url) else {
        return false;
    };
    let host = host.to_ascii_lowercase();

    host.ends_with(".gov")
        || host.ends_with(".mil")
        || host == "dvidshub.net"
        || host == "www.dvidshub.net"
}

pub(crate) fn source_id_from_url(url: &str) -> String {
    let path = url_path(url)
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>()
        .join("/");

    if path.is_empty() {
        "uap-records".to_owned()
    } else {
        path
    }
}

pub(crate) fn detail_url_from_source_id(source_id: &str, base_url: &str) -> Option<String> {
    let trimmed = source_id.trim().trim_matches('/');
    if trimmed.is_empty()
        || trimmed.contains("://")
        || trimmed.contains('?')
        || trimmed.contains('#')
    {
        return None;
    }

    let normalized = if trimmed.contains('/') {
        trimmed.to_owned()
    } else {
        format!("UAP-Records/{trimmed}")
    };

    let encoded = normalized
        .split('/')
        .map(percent_encode_source_id_segment)
        .collect::<Vec<_>>()
        .join("/");

    Some(format!("{}/{}", base_url.trim_end_matches('/'), encoded))
}

pub(crate) fn document_key(source: &str, source_id: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in source.bytes().chain([b':']).chain(source_id.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{source}-{hash:016x}")
}

fn percent_encode_source_id_segment(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut encoded = String::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%'
            && index + 2 < bytes.len()
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            encoded.push('%');
            encoded.push(char::from(bytes[index + 1].to_ascii_uppercase()));
            encoded.push(char::from(bytes[index + 2].to_ascii_uppercase()));
            index += 3;
            continue;
        }

        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
        index += 1;
    }
    encoded
}

fn normalized_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?.to_ascii_lowercase();
    Some(format!("{}://{host}", scheme.to_ascii_lowercase()))
}

fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?;
    Some(format!("{scheme}://{host}"))
}

fn url_host(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?;
    let host = host.split('@').next_back()?;
    Some(host.split(':').next().unwrap_or(host))
}

fn url_path(url: &str) -> String {
    let (_, rest) = url.split_once("://").unwrap_or(("", url));
    let path = rest.split_once('/').map(|(_, tail)| tail).unwrap_or("");
    let path = path.split('#').next().unwrap_or("");
    let path = path.split('?').next().unwrap_or("");
    if path.is_empty() {
        "/".to_owned()
    } else if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}
