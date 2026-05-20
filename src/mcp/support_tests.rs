use crate::{
    index::SearchHit,
    mcp::support::{
        local_document_from_stored, local_document_text_from_stored, local_search_hit_from_index,
        local_search_response_from_index, validate_text_page_range, MAX_TEXT_PAGE_RANGE,
    },
    store::{DocumentKey, StoredDocumentMetadata, StoredPageText},
};

#[test]
fn text_page_range_rejects_missing_bounds_with_actionable_message() {
    let error = validate_text_page_range(None, None).expect_err("missing bounds should fail");

    assert!(error
        .message
        .contains("page_start and page_end are required"));
    assert!(error.message.contains("unbounded full-text retrieval"));
}

#[test]
fn text_page_range_rejects_zero_inverted_and_oversized_ranges() {
    let zero_page = validate_text_page_range(Some(0), Some(1)).expect_err("zero page should fail");
    assert!(zero_page.message.contains("one-based"));

    let inverted =
        validate_text_page_range(Some(2), Some(1)).expect_err("inverted range should fail");
    assert!(inverted
        .message
        .contains("page_start must be less than or equal to page_end"));

    let oversized = validate_text_page_range(Some(1), Some(MAX_TEXT_PAGE_RANGE + 1))
        .expect_err("oversized range should fail");
    assert!(oversized.message.contains("at most 50 pages"));
}

#[test]
fn text_page_range_accepts_one_based_inclusive_maximum() {
    assert_eq!(
        validate_text_page_range(Some(1), Some(MAX_TEXT_PAGE_RANGE)).expect("max range"),
        (1, MAX_TEXT_PAGE_RANGE)
    );
}

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
        response.source_warning.as_deref(),
        Some("DOJ victim-identification warning")
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
    assert_eq!(
        response.source_warning.as_deref(),
        Some("DOJ privacy warning")
    );
    assert_eq!(
        response.citation_note.as_deref(),
        Some("Cite official DOJ page/PDF URL.")
    );
    assert_eq!(
        response.terms_note.as_deref(),
        Some("Sensitive DOJ Epstein Library content.")
    );
    assert!(response.text.contains("Sensitive fixture text"));
    assert!(response.next_actions.is_empty());
}

#[test]
fn local_text_empty_result_includes_guidance_without_body_text() {
    let response = local_document_text_from_stored(
        stored_document_metadata(r#"{"source_metadata":{"source_warning":"DOJ privacy warning"}}"#),
        4,
        4,
        Vec::new(),
    )
    .expect("metadata should parse");

    assert!(response.pages.is_empty());
    assert!(response.text.is_empty());
    assert_eq!(response.next_actions.len(), 1);
    assert!(response.next_actions[0].contains("No local text pages were returned"));
    assert!(response.next_actions[0].contains("one-based page range"));
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
    assert_eq!(hit.source_warning.as_deref(), Some("DOJ source warning"));
    assert_eq!(
        hit.citation_note.as_deref(),
        Some("Cite official DOJ page/PDF URL.")
    );
    assert_eq!(
        hit.terms_note.as_deref(),
        Some("Sensitive DOJ Epstein Library content.")
    );
}

#[test]
fn local_search_response_includes_guidance_for_empty_unfiltered_results() {
    let response = local_search_response_from_index(Vec::new(), None);

    assert!(response.hits.is_empty());
    assert_eq!(response.next_actions.len(), 1);
    assert!(response.next_actions[0].contains("No local hits."));
    assert!(response.next_actions[0].contains("broaden query/source constraints"));
}

#[test]
fn local_search_response_includes_source_specific_guidance_for_filter_miss() {
    let response = local_search_response_from_index(Vec::new(), Some("cia"));

    assert!(response.hits.is_empty());
    assert_eq!(response.next_actions.len(), 1);
    assert!(response.next_actions[0].contains("No local hits for source 'cia'"));
    assert!(response.next_actions[0].contains("broaden query terms"));
}

#[test]
fn local_search_response_keeps_non_empty_hit_shape_and_no_next_actions() {
    let response = local_search_response_from_index(
        vec![SearchHit {
            document_key: DocumentKey::new("doc_cia_search_response").expect("safe key"),
            chunk_id: "chunk-0002".to_owned(),
            source: "cia".to_owned(),
            title: "CIA response-shape fixture".to_owned(),
            page_start: 2,
            page_end: 3,
            score: -2.0,
            snippet: "search response fixture".to_owned(),
            metadata_json: r#"{"source_metadata":{"source_warning":"CIA search warning"}}"#
                .to_owned(),
            citation_note: Some("Cite CIA response fixture.".to_owned()),
            terms_note: Some("CIA response fixture terms.".to_owned()),
        }],
        Some("cia"),
    );

    assert_eq!(response.hits.len(), 1);
    assert!(response.next_actions.is_empty());
    let hit = &response.hits[0];
    assert_eq!(hit.document_key, "doc_cia_search_response");
    assert_eq!(hit.chunk_id, "chunk-0002");
    assert_eq!(hit.source, "cia");
    assert_eq!(hit.title, "CIA response-shape fixture");
    assert_eq!((hit.page_start, hit.page_end), (2, 3));
    assert_eq!(hit.snippet, "search response fixture");
    assert_eq!(
        hit.citation_note.as_deref(),
        Some("Cite CIA response fixture.")
    );
    assert_eq!(
        hit.terms_note.as_deref(),
        Some("CIA response fixture terms.")
    );
    assert_eq!(hit.source_warning.as_deref(), Some("CIA search warning"));
    assert_eq!(hit.warnings, vec!["CIA search warning".to_owned()]);
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
