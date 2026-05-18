pub const SOURCE_NAMES: &[&str] = &[
    "aaro",
    "army",
    "cia",
    "nara",
    "navy",
    "govinfo",
    "pursue",
    "doj_epstein",
    "doj_foia",
    "frus",
    "dtic",
    "dia",
    "noaa",
    "nsa",
    "osd_joint_staff",
    "state",
    "fbi_vault",
];

pub struct SourceRegistryEntry {
    pub name: &'static str,
    pub status: &'static str,
    pub config_note: &'static str,
    pub list_note: &'static str,
}

pub const SOURCE_REGISTRY: &[SourceRegistryEntry] = &[
    SourceRegistryEntry {
        name: "aaro",
        status: "available",
        config_note: "AARO UAP historical-records adapter is available for official aaro.mil records and case-resolution leads; respect source cache headers/rate limits, preserve agency/release metadata, and prefer PDF assets while treating image/video media links as metadata assets.",
        list_note: "AARO adapter is wired for official aaro.mil UAP records and case-resolution leads; respect source cache headers/rate limits, preserve agency/release metadata, and prefer PDF assets while treating media links as metadata assets.",
    },
    SourceRegistryEntry {
        name: "army",
        status: "available",
        config_note: "Army FOIA Reading Room adapter is available for official foia.army.mil leads; respect source cache headers/rate limits, prefer PDF assets, and verify page boundaries before citation.",
        list_note: "Army FOIA Reading Room adapter is wired for official foia.army.mil leads; respect source cache headers/rate limits, prefer PDF assets, and verify page boundaries before citation.",
    },
    SourceRegistryEntry {
        name: "cia",
        status: "available",
        config_note: "CIA Reading Room adapter is available for HTTP search and record fetch; respect source cache headers/rate limits and treat OCR/HTML as uneven lead-finding text until original scans are cited.",
        list_note: "CIA Reading Room adapter is wired for HTTP search and record fetch; respect source cache headers/rate limits and treat OCR/HTML as uneven lead-finding text until original scans are cited.",
    },
    SourceRegistryEntry {
        name: "nara",
        status: "missing_api_key",
        config_note: "Set FOIA_SEARCH_NARA_API_KEY before enabling the NARA adapter; NARA Catalog API responses are DoNotPersist by policy, avoid broad scraping/caching, and observe documented query limits.",
        list_note: "Set FOIA_SEARCH_NARA_API_KEY before calling the NARA adapter; NARA Catalog API responses are DoNotPersist by policy, avoid broad scraping/caching, and observe documented query limits.",
    },
    SourceRegistryEntry {
        name: "navy",
        status: "available",
        config_note: "Navy FOIA Reading Room adapter is available for official secnav.navy.mil Department of the Navy, Naval Audit Service, and Naval Inspector General leads; respect source cache headers/rate limits, prefer PDF assets, and verify page boundaries before citation.",
        list_note: "Navy FOIA Reading Room adapter is wired for official secnav.navy.mil Department of the Navy leads; respect source cache headers/rate limits, prefer PDF assets, and verify page boundaries before citation.",
    },
    SourceRegistryEntry {
        name: "govinfo",
        status: "available",
        config_note: "GovInfo live API adapter is available for Search Service queries and package/granule summary fetches; API response caching respects source headers, callers should observe API rate limits, and PDF/XML/MODS links are preferred.",
        list_note: "GovInfo adapter is wired for Search Service queries and package/granule summary fetches; API response caching respects source headers, callers should observe API rate limits, and PDF/XML/MODS links are preferred.",
    },
    SourceRegistryEntry {
        name: "pursue",
        status: "available",
        config_note: "PURSUE/war.gov UAP release adapter is available for tranche/record leads and official release assets; respect source cache headers/rate limits, PDFs are ingest-preferred, and images/videos remain metadata assets.",
        list_note: "PURSUE/war.gov adapter is wired for release-tranche search and official linked assets; respect source cache headers/rate limits, PDFs are ingest-preferred, and images/videos remain metadata assets.",
    },
    SourceRegistryEntry {
        name: "doj_epstein",
        status: "available",
        config_note: "DOJ Epstein Library adapter is available for official DOJ disclosure leads; respect source cache headers/rate limits, preserve sensitivity/privacy warnings, and prefer PDF ingestion while images/audio/video remain metadata assets.",
        list_note: "DOJ Epstein adapter is wired for official DOJ disclosure leads and detail pages; respect source cache headers/rate limits, preserve sensitive-content warnings, and prefer PDFs over non-PDF media.",
    },
    SourceRegistryEntry {
        name: "doj_foia",
        status: "available",
        config_note: "DOJ component FOIA/disclosure adapter is available from the OIP all-components index; respect source cache headers/rate limits, cite official component pages, and prefer PDF assets while treating HTML/other links as conservative metadata leads.",
        list_note: "DOJ component FOIA/disclosure adapter is wired from the OIP all-components index; respect source cache headers/rate limits, preserve component/category provenance, and cite official component pages or PDFs.",
    },
    SourceRegistryEntry {
        name: "frus",
        status: "available",
        config_note: "FRUS adapter is available for Office of the Historian catalog/detail leads with canonical history.state.gov citations; respect source cache headers/rate limits and prefer official TEI/XML/PDF assets.",
        list_note: "FRUS adapter is wired for official history.state.gov catalog/detail leads; respect source cache headers/rate limits, preserve volume/document citation metadata, and prefer TEI/XML and PDF official assets.",
    },
    SourceRegistryEntry {
        name: "dtic",
        status: "available",
        config_note: "DTIC adapter is available in fragile accession/official-URL tracer mode; broad public search endpoints are not treated as stable APIs, so respect source cache headers/rate limits, preserve distribution warnings, and verify official citation/PDF URLs.",
        list_note: "DTIC adapter is wired in fragile accession/official-URL tracer mode; broad public search endpoints are not treated as stable APIs, so respect source cache headers/rate limits, verify official citation/PDF URLs, and preserve distribution/public-release warnings.",
    },
    SourceRegistryEntry {
        name: "dia",
        status: "available",
        config_note: "DIA FOIA Electronic Reading Room adapter is available for official dia.mil FOIA leads; respect source cache headers/rate limits, prefer PDF assets, and verify page boundaries before citation.",
        list_note: "DIA FOIA Electronic Reading Room adapter is wired for official dia.mil FOIA leads; respect source cache headers/rate limits, prefer PDF assets, and verify page boundaries before citation.",
    },
    SourceRegistryEntry {
        name: "noaa",
        status: "available",
        config_note: "NOAA Institutional Repository adapter is available for official repository.library.noaa.gov publication/report leads; respect source cache headers/rate limits, preserve office/program and DOI/report metadata, and prefer repository PDF assets.",
        list_note: "NOAA Institutional Repository adapter is wired for official repository.library.noaa.gov report/publication leads; respect source cache headers/rate limits, preserve office/program metadata, and prefer repository PDF assets with official item URLs.",
    },
    SourceRegistryEntry {
        name: "nsa",
        status: "available",
        config_note: "NSA FOIA Reading Room adapter is available for official nsa.gov Reading Room and FOIA Reports and Releases leads; respect source cache headers/rate limits, prefer PDF assets, and verify page boundaries before citation.",
        list_note: "NSA FOIA Reading Room adapter is wired for official nsa.gov Reading Room and FOIA Reports and Releases leads; respect source cache headers/rate limits, prefer PDF assets, and verify page boundaries before citation.",
    },
    SourceRegistryEntry {
        name: "osd_joint_staff",
        status: "available",
        config_note: "OSD/Joint Staff FOIA Reading Room adapter is available for official www.esd.whs.mil WHS/ESD leads; respect source cache headers/rate limits, prefer PDF assets, and verify page boundaries before citation.",
        list_note: "OSD/Joint Staff FOIA Reading Room adapter is wired for official www.esd.whs.mil WHS/ESD OSD/Joint Staff FOIA leads; respect source cache headers/rate limits, prefer PDF assets, and verify page boundaries before citation.",
    },
    SourceRegistryEntry {
        name: "state",
        status: "available",
        config_note: "State Department Virtual Reading Room adapter is available for official foia.state.gov Search Released Documents leads; respect source cache headers/rate limits, preserve OCR/originating-agency warnings, prefer PDFs, and verify page boundaries before citation.",
        list_note: "State Department Virtual Reading Room adapter is wired for official foia.state.gov Search Released Documents leads; respect source cache headers/rate limits and preserve OCR, unavailable-field, originating-agency, and page-boundary caveats.",
    },
    SourceRegistryEntry {
        name: "fbi_vault",
        status: "available",
        config_note: "FBI Vault adapter is available for official vault.fbi.gov search/file pages and multipart PDF asset leads; respect source cache headers/rate limits, preserve part-order metadata, and cite official Vault page/PDF URLs.",
        list_note: "FBI Vault adapter is wired for official vault.fbi.gov search and file pages; respect source cache headers/rate limits, preserve multipart part-order metadata, and cite official Vault page/PDF URLs.",
    },
];

pub fn source_registry_entry(name: &str) -> Option<&'static SourceRegistryEntry> {
    SOURCE_REGISTRY.iter().find(|entry| entry.name == name)
}
