use super::{
    run_local_search_eval, FtsSearch, LocalSearchBackend, NamedSearchQuery, NamedSearchQuerySet,
    SearchHit, SearchQuery,
};
use crate::store::{
    ChunkInput, DocumentKey, PageInput, SqliteStore, StoreError, TextSource, UpsertDocument,
};

struct FakeBackend {
    responses: Vec<Vec<SearchHit>>,
}

impl LocalSearchBackend for FakeBackend {
    fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, StoreError> {
        match query.query.as_str() {
            "weather modification" => Ok(self.responses[0].clone()),
            "stormfury" => Ok(self.responses[1].clone()),
            other => panic!("unexpected query {other}"),
        }
    }
}

#[test]
fn eval_report_records_query_order_empty_state_and_shape_parity() {
    let backend = FakeBackend {
        responses: vec![
            vec![
                SearchHit {
                    document_key: DocumentKey::new("doc_cia_weather_001").expect("safe key"),
                    chunk_id: "chunk-1".to_owned(),
                    source: "cia".to_owned(),
                    title: "CIA Weather Control Memo".to_owned(),
                    page_start: 1,
                    page_end: 2,
                    score: -1.25,
                    snippet: "[weather] modification report".to_owned(),
                    metadata_json: r#"{"source_metadata":{"source_warning":"CIA source warning"}}"#
                        .to_owned(),
                    citation_note: Some("Cite CIA PDF.".to_owned()),
                    terms_note: Some("CIA terms.".to_owned()),
                },
                SearchHit {
                    document_key: DocumentKey::new("doc_cia_weather_001").expect("safe key"),
                    chunk_id: "chunk-2".to_owned(),
                    source: "cia".to_owned(),
                    title: "CIA Weather Control Memo".to_owned(),
                    page_start: 3,
                    page_end: 4,
                    score: -0.75,
                    snippet: "[weather] signal review".to_owned(),
                    metadata_json: r#"{"source_warning":"Top-level CIA source warning"}"#
                        .to_owned(),
                    citation_note: Some("Cite CIA PDF.".to_owned()),
                    terms_note: Some("CIA terms.".to_owned()),
                },
            ],
            Vec::new(),
        ],
    };
    let query_set = NamedSearchQuerySet {
        name: "phase9-baseline".to_owned(),
        queries: vec![
            NamedSearchQuery {
                name: "mixed-weather".to_owned(),
                query: SearchQuery {
                    query: "weather modification".to_owned(),
                    source: None,
                    limit: 2,
                },
            },
            NamedSearchQuery {
                name: "empty-stormfury".to_owned(),
                query: SearchQuery {
                    query: "stormfury".to_owned(),
                    source: Some("cia".to_owned()),
                    limit: 5,
                },
            },
        ],
    };

    let report =
        run_local_search_eval("fake-backend", &query_set, &backend).expect("build eval report");

    assert_eq!(report.backend_name, "fake-backend");
    assert_eq!(report.query_set_name, "phase9-baseline");
    assert_eq!(report.query_reports.len(), 2);

    let mixed_weather = &report.query_reports[0];
    assert_eq!(mixed_weather.query_name, "mixed-weather");
    assert_eq!(mixed_weather.hit_count, 2);
    assert!(!mixed_weather.is_empty);
    assert!(mixed_weather.source_filter_matches_all_hits);
    assert_eq!(
        mixed_weather.result_order[0].document_key,
        "doc_cia_weather_001"
    );
    assert_eq!(mixed_weather.result_order[0].chunk_id, "chunk-1");
    assert_eq!(
        mixed_weather.result_order[1].document_key,
        "doc_cia_weather_001"
    );
    assert_eq!(mixed_weather.result_order[1].chunk_id, "chunk-2");

    let first_hit = &mixed_weather.hits[0];
    assert_eq!(first_hit.rank, 1);
    assert_eq!(first_hit.id.chunk_id, "chunk-1");
    assert_eq!(
        first_hit.source_warning.as_deref(),
        Some("CIA source warning")
    );
    assert!(first_hit.shape_parity.has_document_key);
    assert!(first_hit.shape_parity.has_chunk_id);
    assert!(first_hit.shape_parity.has_page_range);
    assert!(first_hit.shape_parity.has_snippet);
    assert!(first_hit.shape_parity.has_citation_note);
    assert!(first_hit.shape_parity.has_terms_note);
    assert!(first_hit.shape_parity.has_source_warning);

    let second_hit = &mixed_weather.hits[1];
    assert_eq!(second_hit.rank, 2);
    assert_eq!(second_hit.id.document_key, "doc_cia_weather_001");
    assert_eq!(second_hit.id.chunk_id, "chunk-2");
    assert_eq!(
        second_hit.source_warning.as_deref(),
        Some("Top-level CIA source warning")
    );
    assert_eq!((second_hit.page_start, second_hit.page_end), (3, 4));
    assert!(second_hit.shape_parity.has_source_warning);

    let empty_stormfury = &report.query_reports[1];
    assert_eq!(empty_stormfury.query_name, "empty-stormfury");
    assert!(empty_stormfury.is_empty);
    assert_eq!(empty_stormfury.hit_count, 0);
    assert!(empty_stormfury.source_filter_matches_all_hits);
    assert!(empty_stormfury.hits.is_empty());
    assert!(empty_stormfury.result_order.is_empty());
}

#[test]
fn eval_report_flags_source_filter_mismatches_without_hiding_results() {
    let backend = FakeBackend {
        responses: vec![
            vec![SearchHit {
                document_key: DocumentKey::new("doc_nsa_weather_001").expect("safe key"),
                chunk_id: "chunk-9".to_owned(),
                source: "nsa".to_owned(),
                title: "NSA Weather Notes".to_owned(),
                page_start: 1,
                page_end: 1,
                score: -0.5,
                snippet: "[weather] note".to_owned(),
                metadata_json: "{}".to_owned(),
                citation_note: Some("Cite NSA PDF.".to_owned()),
                terms_note: Some("NSA terms.".to_owned()),
            }],
            Vec::new(),
        ],
    };
    let query_set = NamedSearchQuerySet {
        name: "phase9-filter-check".to_owned(),
        queries: vec![
            NamedSearchQuery {
                name: "bad-filter".to_owned(),
                query: SearchQuery {
                    query: "weather modification".to_owned(),
                    source: Some("cia".to_owned()),
                    limit: 1,
                },
            },
            NamedSearchQuery {
                name: "empty-stormfury".to_owned(),
                query: SearchQuery {
                    query: "stormfury".to_owned(),
                    source: None,
                    limit: 1,
                },
            },
        ],
    };

    let report =
        run_local_search_eval("fake-backend", &query_set, &backend).expect("build eval report");

    let mismatch = &report.query_reports[0];
    assert_eq!(mismatch.hit_count, 1);
    assert!(!mismatch.is_empty);
    assert!(!mismatch.source_filter_matches_all_hits);
    assert_eq!(mismatch.hits[0].source, "nsa");
}

#[test]
fn eval_report_reads_source_warning_from_nested_and_top_level_metadata_shapes() {
    let backend = FakeBackend {
        responses: vec![
            vec![
                SearchHit {
                    document_key: DocumentKey::new("doc_cia_weather_010").expect("safe key"),
                    chunk_id: "chunk-a".to_owned(),
                    source: "cia".to_owned(),
                    title: "Nested warning".to_owned(),
                    page_start: 1,
                    page_end: 1,
                    score: -1.0,
                    snippet: "[weather] nested".to_owned(),
                    metadata_json: r#"{"source_metadata":{"source_warning":"Nested warning"}}"#
                        .to_owned(),
                    citation_note: Some("Cite nested.".to_owned()),
                    terms_note: Some("Nested terms.".to_owned()),
                },
                SearchHit {
                    document_key: DocumentKey::new("doc_cia_weather_011").expect("safe key"),
                    chunk_id: "chunk-b".to_owned(),
                    source: "cia".to_owned(),
                    title: "Top-level warning".to_owned(),
                    page_start: 2,
                    page_end: 2,
                    score: -0.5,
                    snippet: "[weather] top level".to_owned(),
                    metadata_json: r#"{"source_warning":"Top-level warning"}"#.to_owned(),
                    citation_note: Some("Cite top-level.".to_owned()),
                    terms_note: Some("Top-level terms.".to_owned()),
                },
            ],
            Vec::new(),
        ],
    };
    let query_set = NamedSearchQuerySet {
        name: "warning-shapes".to_owned(),
        queries: vec![
            NamedSearchQuery {
                name: "warning-coverage".to_owned(),
                query: SearchQuery {
                    query: "weather modification".to_owned(),
                    source: Some("cia".to_owned()),
                    limit: 10,
                },
            },
            NamedSearchQuery {
                name: "empty-stormfury".to_owned(),
                query: SearchQuery {
                    query: "stormfury".to_owned(),
                    source: None,
                    limit: 1,
                },
            },
        ],
    };

    let report =
        run_local_search_eval("fake-backend", &query_set, &backend).expect("build eval report");

    let hits = &report.query_reports[0].hits;
    assert_eq!(hits[0].source_warning.as_deref(), Some("Nested warning"));
    assert!(hits[0].shape_parity.has_source_warning);
    assert_eq!(hits[1].source_warning.as_deref(), Some("Top-level warning"));
    assert!(hits[1].shape_parity.has_source_warning);
}

#[test]
fn eval_report_integrates_with_sqlite_fts_backend() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    seed_fixture(
        &mut store,
        FixtureDoc {
            document_key: "doc_cia_weather_003",
            public_id: "cia:weather-003",
            source: "cia",
            title: "CIA Weather Eval Fixture",
            metadata_json: r#"{"source_metadata":{"source_warning":"CIA eval warning"}}"#,
            citation_note: Some("Cite CIA eval PDF."),
            terms_note: Some("CIA eval terms."),
            chunk_id: "chunk-003",
            page_start: 9,
            page_end: 10,
            chunk_text: "weather modification field evaluation",
        },
    );

    let query_set = NamedSearchQuerySet {
        name: "sqlite-phase9".to_owned(),
        queries: vec![NamedSearchQuery {
            name: "cia-weather".to_owned(),
            query: SearchQuery {
                query: "weather modification".to_owned(),
                source: Some("cia".to_owned()),
                limit: 5,
            },
        }],
    };

    let report = run_local_search_eval("sqlite-fts", &query_set, &FtsSearch::new(&store))
        .expect("build sqlite eval report");

    let query_report = &report.query_reports[0];
    assert_eq!(query_report.hit_count, 1);
    assert!(query_report.source_filter_matches_all_hits);
    assert_eq!(
        query_report.result_order[0].document_key,
        "doc_cia_weather_003"
    );
    assert_eq!(
        query_report.hits[0].source_warning.as_deref(),
        Some("CIA eval warning")
    );
    assert_eq!(query_report.hits[0].page_start, 9);
    assert_eq!(query_report.hits[0].page_end, 10);
}

struct FixtureDoc<'a> {
    document_key: &'a str,
    public_id: &'a str,
    source: &'a str,
    title: &'a str,
    metadata_json: &'a str,
    citation_note: Option<&'a str>,
    terms_note: Option<&'a str>,
    chunk_id: &'a str,
    page_start: i64,
    page_end: i64,
    chunk_text: &'a str,
}

fn seed_fixture(store: &mut SqliteStore, doc: FixtureDoc<'_>) {
    let key = DocumentKey::new(doc.document_key).expect("safe fixture key");
    let source_id = doc
        .public_id
        .split_once(':')
        .expect("fixture ids must be source-prefixed")
        .1
        .to_owned();

    store
        .upsert_document(&UpsertDocument {
            public_id: doc.public_id.to_owned(),
            document_key: key.clone(),
            source: doc.source.to_owned(),
            source_id,
            title: doc.title.to_owned(),
            date: Some("1960-01-01".to_owned()),
            collection: Some("Phase 9 eval fixtures".to_owned()),
            record_group: None,
            description: Some("Deterministic SQLite local-search eval fixture".to_owned()),
            origin_url: None,
            document_url: None,
            pdf_url: None,
            metadata_json: doc.metadata_json.to_owned(),
            citation_note: doc.citation_note.map(str::to_owned),
            terms_note: doc.terms_note.map(str::to_owned),
        })
        .expect("insert fixture document");
    store
        .replace_pages_and_chunks(
            &key,
            &[PageInput {
                document_key: key.clone(),
                page_number: doc.page_start,
                text: doc.chunk_text.to_owned(),
                text_source: TextSource::EmbeddedPdfText,
                quality_score: Some(0.9),
                warnings_json: "[]".to_owned(),
            }],
            &[ChunkInput {
                document_key: key.clone(),
                chunk_id: doc.chunk_id.to_owned(),
                page_start: doc.page_start,
                page_end: doc.page_end,
                text: doc.chunk_text.to_owned(),
                token_estimate: Some(doc.chunk_text.split_whitespace().count() as i64),
                metadata_json: "{}".to_owned(),
            }],
        )
        .expect("replace fixture pages and chunks");
}
