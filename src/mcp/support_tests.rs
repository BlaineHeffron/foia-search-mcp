use crate::{
    index::SearchHit,
    mcp::support::{
        local_document_from_stored, local_document_text_from_stored, local_search_hit_from_index,
    },
    store::{DocumentKey, StoredDocumentMetadata, StoredPageText},
};

#[test]
fn local_document_outputs_source_warning_from_persisted_metadata() {
    let response = local_document_from_stored(stored_document_metadata(
        r#"{"source_metadata":{"source_warning":"DOJ victim-identification warning"}}"#,
    ))
    .expect("metadata should parse");

    assert_eq!(
        response.warnings,
        vec!["DOJ victim-identification warning".to_owned()]
    );
    assert_eq!(
        response.citation_note.as_deref(),
        Some("Cite official DOJ page/PDF URL.")
    );
    assert_eq!(
        response.terms_note.as_deref(),
        Some("Sensitive DOJ Epstein Library content.")
    );
}

#[test]
fn local_text_outputs_source_warning_from_persisted_metadata() {
    let response = local_document_text_from_stored(
        stored_document_metadata(r#"{"source_metadata":{"source_warning":"DOJ privacy warning"}}"#),
        1,
        1,
        vec![StoredPageText {
            page_number: 1,
            text: "Sensitive fixture text".to_owned(),
            text_source: "embedded_pdf_text".to_owned(),
        }],
    )
    .expect("metadata should parse");

    assert_eq!(response.warnings, vec!["DOJ privacy warning".to_owned()]);
    assert!(response.text.contains("Sensitive fixture text"));
}

#[test]
fn local_search_hit_outputs_source_warning_citation_and_terms() {
    let hit = local_search_hit_from_index(SearchHit {
        document_key: DocumentKey::new("doc_doj_epstein_001").expect("safe key"),
        chunk_id: "chunk-0001".to_owned(),
        source: "doj_epstein".to_owned(),
        title: "DOJ Epstein fixture".to_owned(),
        page_start: 1,
        page_end: 1,
        score: -1.0,
        snippet: "fixture".to_owned(),
        metadata_json: r#"{"source_metadata":{"source_warning":"DOJ source warning"}}"#.to_owned(),
        citation_note: Some("Cite official DOJ page/PDF URL.".to_owned()),
        terms_note: Some("Sensitive DOJ Epstein Library content.".to_owned()),
    });

    assert_eq!(hit.warnings, vec!["DOJ source warning".to_owned()]);
    assert_eq!(
        hit.citation_note.as_deref(),
        Some("Cite official DOJ page/PDF URL.")
    );
    assert_eq!(
        hit.terms_note.as_deref(),
        Some("Sensitive DOJ Epstein Library content.")
    );
}

fn stored_document_metadata(metadata_json: &str) -> StoredDocumentMetadata {
    StoredDocumentMetadata {
        id: 1,
        public_id: "doj_epstein:data-set-1-files".to_owned(),
        document_key: DocumentKey::new("doc_doj_epstein_001").expect("safe key"),
        source: "doj_epstein".to_owned(),
        source_id: "data-set-1-files".to_owned(),
        title: "DOJ Epstein fixture".to_owned(),
        date: None,
        collection: Some("DOJ Epstein Library".to_owned()),
        record_group: None,
        description: None,
        origin_url: Some("https://www.justice.gov/epstein/doj-disclosures".to_owned()),
        document_url: Some(
            "https://www.justice.gov/epstein/doj-disclosures/data-set-1-files".to_owned(),
        ),
        pdf_url: Some("https://www.justice.gov/epstein/files/report.pdf".to_owned()),
        metadata_json: metadata_json.to_owned(),
        citation_note: Some("Cite official DOJ page/PDF URL.".to_owned()),
        terms_note: Some("Sensitive DOJ Epstein Library content.".to_owned()),
        page_count: 1,
    }
}
