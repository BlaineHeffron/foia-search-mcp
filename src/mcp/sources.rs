use rmcp::ErrorData as McpError;

use crate::errors::FoiaSearchError;

pub(crate) const VALID_SOURCES: &[&str] = &[
    "aaro",
    "cia",
    "nara",
    "govinfo",
    "pursue",
    "doj_epstein",
    "doj_foia",
    "frus",
    "dtic",
    "noaa",
    "nsa",
    "state",
    "fbi_vault",
];

pub(crate) fn validate_source_name(source: &str) -> Result<(), McpError> {
    if VALID_SOURCES.contains(&source) {
        Ok(())
    } else {
        Err(FoiaSearchError::InvalidRequest(format!(
            "invalid source '{}'; expected one of: {}",
            source,
            VALID_SOURCES.join(", ")
        ))
        .into_mcp_error())
    }
}

pub(crate) fn list_sources_note(adapter_name: &str, enabled: bool) -> Option<String> {
    match adapter_name {
        "aaro" => Some(
            "AARO adapter is wired for official aaro.mil UAP records and case-resolution leads; preserve agency/release metadata and prefer PDF assets while treating media links as metadata assets."
                .to_owned(),
        ),
        "cia" => Some("CIA Reading Room adapter is wired for HTTP search and record fetch.".to_owned()),
        "nara" if enabled => Some(
            "NARA Catalog adapter is wired for API-key HTTP search and record fetch; persistent API response caching is disabled by policy."
                .to_owned(),
        ),
        "nara" => Some("Set FOIA_SEARCH_NARA_API_KEY before calling the NARA adapter.".to_owned()),
        "govinfo" => Some(
            "GovInfo adapter is wired for Search Service queries and package/granule summary fetches; API response caching follows source headers."
                .to_owned(),
        ),
        "pursue" => Some(
            "PURSUE/war.gov adapter is wired for release-tranche search and official linked assets; PDFs are ingest-preferred while images/videos remain metadata assets."
                .to_owned(),
        ),
        "doj_epstein" => Some(
            "DOJ Epstein adapter is wired for official DOJ disclosure leads and detail pages; sensitive-content warnings must be preserved and PDFs remain ingest-preferred over non-PDF media."
                .to_owned(),
        ),
        "doj_foia" => Some(
            "DOJ component FOIA/disclosure adapter is wired from the OIP all-components index; preserve component/category provenance and cite official component pages or PDFs."
                .to_owned(),
        ),
        "frus" => Some(
            "FRUS adapter is wired for official history.state.gov catalog/detail leads; preserve volume/document citation metadata and prefer TEI/XML and PDF official assets."
                .to_owned(),
        ),
        "dtic" => Some(
            "DTIC adapter is wired in fragile accession/official-URL tracer mode; broad public search endpoints are not treated as stable APIs, so verify official citation/PDF URLs and preserve distribution/public-release warnings."
                .to_owned(),
        ),
        "fbi_vault" => Some(
            "FBI Vault adapter is wired for official vault.fbi.gov search and file pages; preserve multipart part-order metadata and cite official Vault page/PDF URLs."
                .to_owned(),
        ),
        "noaa" => Some(
            "NOAA Institutional Repository adapter is wired for official repository.library.noaa.gov report/publication leads; preserve office/program metadata and prefer repository PDF assets with official item URLs."
                .to_owned(),
        ),
        "nsa" => Some(
            "NSA FOIA Reading Room adapter is wired for official nsa.gov Reading Room and FOIA Reports and Releases leads; prefer PDF assets and verify page boundaries before citation."
                .to_owned(),
        ),
        "state" => Some(
            "State Department Virtual Reading Room adapter is wired for official foia.state.gov Search Released Documents leads; preserve OCR, unavailable-field, originating-agency, and page-boundary caveats."
                .to_owned(),
        ),
        _ => None,
    }
}
