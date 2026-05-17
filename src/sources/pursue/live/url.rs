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
        let origin = base
            .split_once("://")
            .and_then(|(scheme, rest)| {
                rest.split('/')
                    .next()
                    .map(|host| format!("{scheme}://{host}"))
            })
            .unwrap_or_else(|| base.to_owned());
        return format!("{origin}{href}");
    }
    format!("{base}/{href}")
}

pub(crate) fn is_allowed_war_url(url: &str, base_url: &str) -> bool {
    url_origin(url)
        .zip(url_origin(base_url))
        .map(|(url_origin, base_origin)| url_origin == base_origin)
        .unwrap_or(false)
}

pub(crate) fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub(crate) fn document_key(source: &str, source_id: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in source.bytes().chain([b':']).chain(source_id.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{source}-{hash:016x}")
}

fn url_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?.to_ascii_lowercase();
    Some(format!("{}://{host}", scheme.to_ascii_lowercase()))
}
