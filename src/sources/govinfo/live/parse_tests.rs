use super::parse::{parse_locator, record_from_search_result};
use super::{govinfo_citation_note, govinfo_terms_note, GovInfoLocator};

#[test]
fn parse_locator_accepts_ids_urls_and_composites() {
    let package = parse_locator("USREPORTS-99").expect("package id should parse");
    assert!(matches!(
        package,
        GovInfoLocator::Package { package_id } if package_id == "USREPORTS-99"
    ));

    let granule = parse_locator("USREPORTS-99/USREPORTS-99-FrontMatter-2")
        .expect("granule composite should parse");
    assert!(matches!(
        granule,
        GovInfoLocator::Granule {
            package_id,
            granule_id
        } if package_id == "USREPORTS-99" && granule_id == "USREPORTS-99-FrontMatter-2"
    ));

    let api_url = parse_locator(
        "https://api.govinfo.gov/packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg36/summary",
    )
    .expect("API summary URL should parse");
    assert!(matches!(
        api_url,
        GovInfoLocator::Granule {
            package_id,
            granule_id
        } if package_id == "WCPD-2009-01-19" && granule_id == "WCPD-2009-01-19-Pg36"
    ));

    let details_url = parse_locator("https://www.govinfo.gov/app/details/USREPORTS-99")
        .expect("details URL should parse");
    assert!(matches!(
        details_url,
        GovInfoLocator::Package { package_id } if package_id == "USREPORTS-99"
    ));
}

#[test]
fn search_record_normalization_prefers_result_link_and_notes() {
    let result = serde_json::json!({
        "title": "Interview With Brit Hume of FOX News",
        "packageId": "WCPD-2009-01-19",
        "granuleId": "WCPD-2009-01-19-Pg36",
        "dateIssued": "2009-01-07",
        "collectionCode": "CPD",
        "resultLink": "https://api.govinfo.gov/packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg36/summary",
        "download": {
            "pdfLink": "https://api.govinfo.gov/packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg36/pdf",
            "modsLink": "https://api.govinfo.gov/packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg36/mods"
        }
    });

    let record = record_from_search_result(&result).expect("search result should parse");
    assert_eq!(record.source_id, "WCPD-2009-01-19/WCPD-2009-01-19-Pg36");
    assert!(record
        .document_url
        .contains("/granules/WCPD-2009-01-19-Pg36/summary"));
    assert_eq!(
        record.pdf_url.as_deref(),
        Some("https://api.govinfo.gov/packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg36/pdf")
    );
    assert_eq!(
        record.citation_note.as_deref(),
        Some(govinfo_citation_note())
    );
    assert_eq!(record.terms_note.as_deref(), Some(govinfo_terms_note()));
}
