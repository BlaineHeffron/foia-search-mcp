pub(crate) fn collection_for_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("/navaudsvc/") {
        "Naval Audit Service FOIA Reading Room"
    } else if lower.contains("/ig/") {
        "Naval Inspector General FOIA Reading Room"
    } else {
        "Department of the Navy FOIA Reading Room"
    }
}

pub(crate) fn record_group_for_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("/navaudsvc/") {
        "naval_audit_service"
    } else if lower.contains("/ig/") {
        "naval_inspector_general"
    } else {
        "department_of_the_navy_foia_reading_room"
    }
}

pub(crate) fn description_for_url(url: &str, has_pdf: bool) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("scorpion") {
        "Official Department of the Navy Scorpion Submarine FOIA release lead."
    } else if lower.contains("red%20hill") || lower.contains("red hill") {
        "Official Department of the Navy Red Hill FOIA release lead."
    } else if lower.contains("/navaudsvc/") {
        "Official Naval Audit Service FOIA reading-room lead."
    } else if lower.contains("/ig/") {
        "Official Naval Inspector General FOIA reading-room lead."
    } else if has_pdf {
        "Official Department of the Navy FOIA PDF lead."
    } else {
        "Official Department of the Navy FOIA reading-room lead."
    }
}
