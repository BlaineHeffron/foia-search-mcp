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

    let directory = base.rsplit_once('/').map(|(head, _)| head).unwrap_or(base);
    format!("{directory}/{href}")
}

pub(crate) fn canonicalize_official_url(url: &str, base_url: &str) -> String {
    let Some(url_host_name) = url_host(url) else {
        return url.to_owned();
    };
    let Some(base_host) = url_host(base_url) else {
        return url.to_owned();
    };

    if url_host_name.eq_ignore_ascii_case("esd.whs.mil")
        && base_host.eq_ignore_ascii_case("www.esd.whs.mil")
    {
        return url.replacen("://esd.whs.mil", "://www.esd.whs.mil", 1);
    }
    if url_host_name.eq_ignore_ascii_case("www.esd.whs.mil")
        && base_host.eq_ignore_ascii_case("www.esd.whs.mil")
        && url.starts_with("http://")
    {
        return format!("https://{}", &url["http://".len()..]);
    }
    url.to_owned()
}

pub(crate) fn is_allowed_osd_joint_staff_url(url: &str, base_url: &str) -> bool {
    let Some(url_origin) = normalized_origin(url) else {
        return false;
    };
    let Some(base_origin) = normalized_origin(base_url) else {
        return false;
    };
    if url_origin != base_origin {
        return false;
    }

    let path = url_path(url).to_ascii_lowercase();
    !path.trim_matches('/').is_empty()
        && (path.starts_with("/foid/")
            || path.starts_with("/foia/")
            || path.starts_with("/records-declass/foia/reading-room/")
            || path.starts_with("/portals/")
            || path.ends_with(".pdf")
            || path.contains("/reading%20room/"))
}

pub(crate) fn source_id_from_url(url: &str) -> String {
    let path = url_path(url)
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>()
        .join("/");
    let query = url_query(url);

    match (path.is_empty(), query.is_empty()) {
        (true, _) => "Records-Declass/FOIA/Reading-Room/Reading-Room-List_2".to_owned(),
        (false, true) => path,
        (false, false) => format!("{path}?{query}"),
    }
}

pub(crate) fn detail_url_from_source_id(source_id: &str, base_url: &str) -> Option<String> {
    let trimmed = source_id.trim().trim_matches('/');
    if trimmed.is_empty() || trimmed.contains("://") || trimmed.contains('#') {
        return None;
    }

    let (path, query) = trimmed
        .split_once('?')
        .map(|(path, query)| (path, Some(query)))
        .unwrap_or((trimmed, None));
    if path.is_empty() {
        return None;
    }

    let encoded_path = path
        .split('/')
        .map(percent_encode_path_segment)
        .collect::<Vec<_>>()
        .join("/");
    let mut url = format!("{}/{}", base_url.trim_end_matches('/'), encoded_path);
    if let Some(query) = query.filter(|value| !value.trim().is_empty()) {
        url.push('?');
        url.push_str(query);
    }
    Some(url)
}

pub(crate) fn document_key(source: &str, source_id: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in source.bytes().chain([b':']).chain(source_id.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{source}-{hash:016x}")
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%'
            && index + 2 < bytes.len()
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            encoded.push('%');
            encoded.push(char::from(bytes[index + 1]));
            encoded.push(char::from(bytes[index + 2]));
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

fn url_query(url: &str) -> String {
    let (_, rest) = url.split_once("://").unwrap_or(("", url));
    rest.split_once('?')
        .map(|(_, query)| query.split('#').next().unwrap_or("").to_owned())
        .unwrap_or_default()
}
