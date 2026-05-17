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

pub(crate) fn is_allowed_component_url(url: &str, index_url: &str) -> bool {
    let Some(host) = url_host(url) else {
        return false;
    };
    let host = host.to_ascii_lowercase();

    let allowed_by_same_origin = url_origin(url)
        .zip(url_origin(index_url))
        .map(|(url_origin, index_origin)| url_origin == index_origin)
        .unwrap_or(false);
    if allowed_by_same_origin {
        return true;
    }

    host == "justice.gov"
        || host.ends_with(".justice.gov")
        || host == "usdoj.gov"
        || host.ends_with(".usdoj.gov")
        || host == "atf.gov"
        || host.ends_with(".atf.gov")
        || host == "dea.gov"
        || host.ends_with(".dea.gov")
        || host == "bop.gov"
        || host.ends_with(".bop.gov")
        || host == "fbi.gov"
        || host.ends_with(".fbi.gov")
        || host == "usmarshals.gov"
        || host.ends_with(".usmarshals.gov")
        || host == "ojp.gov"
        || host.ends_with(".ojp.gov")
}

pub(crate) fn source_id_from_component_name(component: &str) -> String {
    let slug = slugify(component);
    if slug.is_empty() {
        "doj-component".to_owned()
    } else {
        slug
    }
}

pub(crate) fn source_id_from_url(url: &str) -> String {
    let (_, rest) = url.split_once("://").unwrap_or(("", url));
    let path = rest.split_once('/').map(|(_, tail)| tail).unwrap_or("");
    let path = path.split(['?', '#']).next().unwrap_or("");
    let slug = slugify(path);
    if slug.is_empty() {
        "doj-component".to_owned()
    } else {
        slug
    }
}

pub(crate) fn document_key(source: &str, source_id: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in source.bytes().chain([b':']).chain(source_id.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{source}-{hash:016x}")
}

pub(crate) fn slugify(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn url_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?.to_ascii_lowercase();
    Some(format!("{}://{host}", scheme.to_ascii_lowercase()))
}

fn url_host(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?;
    let host = host.split('@').next_back()?;
    Some(host.split(':').next().unwrap_or(host))
}
