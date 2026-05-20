use super::{run_local_search_eval, FtsSearch, NamedSearchQuery, NamedSearchQuerySet, SearchQuery};
use crate::store::{ChunkInput, DocumentKey, PageInput, SqliteStore, TextSource, UpsertDocument};

#[test]
fn sqlite_sufficiency_eval_covers_mixed_source_filter_and_shape_contract() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    seed_phase9_fixture(&mut store);

    let report = run_local_search_eval(
        "sqlite-fts",
        &phase9_sufficiency_query_set(),
        &FtsSearch::new(&store),
    )
    .expect("build sqlite sufficiency eval report");

    assert_eq!(report.backend_name, "sqlite-fts");
    assert_eq!(report.query_set_name, "phase9-sqlite-sufficiency");
    assert_eq!(report.query_reports.len(), 3);

    let mixed = &report.query_reports[0];
    assert_eq!(mixed.query_name, "mixed-source-top-k");
    assert_eq!(mixed.hit_count, 2);
    assert!(!mixed.is_empty);
    assert!(mixed.empty_result_next_action.is_none());
    assert_eq!(
        hit_ids(mixed),
        ["doc_cia_weather_010", "doc_nsa_weather_010"]
    );
    assert_ne!(
        mixed.hits[0].score, mixed.hits[1].score,
        "fixture uses distinct scores; tie-order determinism is a separate follow-up contract"
    );

    let top_hit = &mixed.hits[0];
    assert_eq!(top_hit.source, "cia");
    assert_eq!(top_hit.page_start, 1);
    assert_eq!(top_hit.page_end, 2);
    assert_eq!(
        top_hit.citation_note.as_deref(),
        Some("Cite CIA fixture PDF.")
    );
    assert_eq!(top_hit.terms_note.as_deref(), Some("CIA fixture terms."));
    assert_eq!(
        top_hit.source_warning.as_deref(),
        Some("CIA nested fixture warning")
    );
    assert!(top_hit.shape_parity.has_document_key);
    assert!(top_hit.shape_parity.has_chunk_id);
    assert!(top_hit.shape_parity.has_page_range);
    assert!(top_hit.shape_parity.has_snippet);
    assert!(top_hit.shape_parity.has_citation_note);
    assert!(top_hit.shape_parity.has_terms_note);
    assert!(top_hit.shape_parity.has_source_warning);

    let second_hit = &mixed.hits[1];
    assert_eq!(second_hit.source, "nsa");
    assert_eq!(
        second_hit.source_warning.as_deref(),
        Some("NSA top-level fixture warning")
    );

    let filtered = &report.query_reports[1];
    assert_eq!(filtered.query_name, "source-filter-cia");
    assert_eq!(filtered.hit_count, 1);
    assert!(!filtered.is_empty);
    assert!(filtered.empty_result_next_action.is_none());
    assert!(filtered.source_filter_matches_all_hits);
    assert_eq!(hit_ids(filtered), ["doc_cia_weather_010"]);
    assert_eq!(filtered.hits[0].source, "cia");

    let sparse = &report.query_reports[2];
    assert_eq!(sparse.query_name, "sparse-no-match");
    assert_eq!(sparse.hit_count, 0);
    assert!(sparse.is_empty);
    assert_eq!(
        sparse.empty_result_next_action.as_deref(),
        Some(
            "No local hits for source 'cia'. Ingest additional local documents for that source or broaden query terms."
        )
    );
    assert!(sparse.source_filter_matches_all_hits);
    assert!(sparse.hits.is_empty());
}

#[test]
fn sqlite_sufficiency_eval_keeps_mixed_source_top_k_order_stable_across_runs() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    seed_phase9_fixture(&mut store);

    let query_set = NamedSearchQuerySet {
        name: "phase9-sqlite-top-k".to_owned(),
        queries: vec![NamedSearchQuery {
            name: "mixed-source-top-k".to_owned(),
            query: SearchQuery {
                query: "weather modification".to_owned(),
                source: None,
                limit: 2,
            },
        }],
    };

    let first = run_local_search_eval("sqlite-fts", &query_set, &FtsSearch::new(&store))
        .expect("run first eval pass");
    let second = run_local_search_eval("sqlite-fts", &query_set, &FtsSearch::new(&store))
        .expect("run second eval pass");

    let first_order = hit_ids(&first.query_reports[0]);
    let second_order = hit_ids(&second.query_reports[0]);
    assert_eq!(first_order, second_order);
    assert_eq!(first_order, ["doc_cia_weather_010", "doc_nsa_weather_010"]);
}

#[test]
fn sqlite_sufficiency_eval_reports_empty_corpus_query_as_empty() {
    let store = SqliteStore::open_memory().expect("open empty in-memory store");
    let query_set = NamedSearchQuerySet {
        name: "phase9-sqlite-empty-corpus".to_owned(),
        queries: vec![NamedSearchQuery {
            name: "weather-on-empty-corpus".to_owned(),
            query: SearchQuery {
                query: "weather modification".to_owned(),
                source: None,
                limit: 5,
            },
        }],
    };

    let report = run_local_search_eval("sqlite-fts", &query_set, &FtsSearch::new(&store))
        .expect("build empty-corpus eval report");
    let empty = &report.query_reports[0];

    assert_eq!(empty.query_name, "weather-on-empty-corpus");
    assert_eq!(empty.hit_count, 0);
    assert!(empty.is_empty);
    assert_eq!(
        empty.empty_result_next_action.as_deref(),
        Some("No local hits. Ingest local documents first or broaden query/source constraints.")
    );
    assert!(empty.result_order.is_empty());
    assert!(empty.hits.is_empty());
    assert!(empty.source_filter_matches_all_hits);
}

#[test]
fn sqlite_sufficiency_eval_uses_deterministic_tie_breaker_for_equal_scores() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    seed_fixture(
        &mut store,
        FixtureDoc {
            document_key: "doc_cia_tie_001",
            public_id: "cia:tie-001",
            source: "cia",
            title: "CIA Tie Fixture",
            metadata_json: r#"{"source_metadata":{"source_warning":"CIA tie fixture warning"}}"#,
            citation_note: Some("Cite CIA tie fixture PDF."),
            terms_note: Some("CIA tie fixture terms."),
            chunk_id: "chunk-tie-001",
            page_start: 1,
            page_end: 1,
            chunk_text: "atmospheric lensing fixture phrase",
        },
    );
    seed_fixture(
        &mut store,
        FixtureDoc {
            document_key: "doc_nsa_tie_001",
            public_id: "nsa:tie-001",
            source: "nsa",
            title: "NSA Tie Fixture",
            metadata_json: r#"{"source_metadata":{"source_warning":"NSA tie fixture warning"}}"#,
            citation_note: Some("Cite NSA tie fixture PDF."),
            terms_note: Some("NSA tie fixture terms."),
            chunk_id: "chunk-tie-001",
            page_start: 1,
            page_end: 1,
            chunk_text: "atmospheric lensing fixture phrase",
        },
    );

    let query_set = NamedSearchQuerySet {
        name: "phase9-sqlite-score-tie".to_owned(),
        queries: vec![NamedSearchQuery {
            name: "score-tie-order".to_owned(),
            query: SearchQuery {
                query: "atmospheric lensing".to_owned(),
                source: None,
                limit: 2,
            },
        }],
    };

    let report = run_local_search_eval("sqlite-fts", &query_set, &FtsSearch::new(&store))
        .expect("build score-tie eval report");
    let tie = &report.query_reports[0];

    assert_eq!(tie.hit_count, 2);
    assert_eq!(tie.hits[0].score, tie.hits[1].score);
    assert_eq!(hit_ids(tie), ["doc_cia_tie_001", "doc_nsa_tie_001"]);
}

fn phase9_sufficiency_query_set() -> NamedSearchQuerySet {
    NamedSearchQuerySet {
        name: "phase9-sqlite-sufficiency".to_owned(),
        queries: vec![
            NamedSearchQuery {
                name: "mixed-source-top-k".to_owned(),
                query: SearchQuery {
                    query: "weather modification".to_owned(),
                    source: None,
                    limit: 2,
                },
            },
            NamedSearchQuery {
                name: "source-filter-cia".to_owned(),
                query: SearchQuery {
                    query: "weather modification".to_owned(),
                    source: Some("cia".to_owned()),
                    limit: 5,
                },
            },
            NamedSearchQuery {
                name: "sparse-no-match".to_owned(),
                query: SearchQuery {
                    query: "stormfury".to_owned(),
                    source: Some("cia".to_owned()),
                    limit: 5,
                },
            },
        ],
    }
}

fn seed_phase9_fixture(store: &mut SqliteStore) {
    seed_fixture(
        store,
        FixtureDoc {
            document_key: "doc_cia_weather_010",
            public_id: "cia:weather-010",
            source: "cia",
            title: "CIA Weather Control Memo",
            metadata_json: r#"{"source_metadata":{"source_warning":"CIA nested fixture warning"}}"#,
            citation_note: Some("Cite CIA fixture PDF."),
            terms_note: Some("CIA fixture terms."),
            chunk_id: "chunk-0010",
            page_start: 1,
            page_end: 2,
            chunk_text: "weather modification weather modification cloud seeding field report",
        },
    );
    seed_fixture(
        store,
        FixtureDoc {
            document_key: "doc_nsa_weather_010",
            public_id: "nsa:weather-010",
            source: "nsa",
            title: "NSA Weather Notes",
            metadata_json: r#"{"source_warning":"NSA top-level fixture warning"}"#,
            citation_note: Some("Cite NSA fixture PDF."),
            terms_note: Some("NSA fixture terms."),
            chunk_id: "chunk-0010",
            page_start: 3,
            page_end: 4,
            chunk_text: "weather modification signal review",
        },
    );
    seed_fixture(
        store,
        FixtureDoc {
            document_key: "doc_dia_weather_010",
            public_id: "dia:weather-010",
            source: "dia",
            title: "DIA Weather Digest",
            metadata_json: r#"{"source_metadata":{"source_warning":"DIA fixture warning"}}"#,
            citation_note: Some("Cite DIA fixture PDF."),
            terms_note: Some("DIA fixture terms."),
            chunk_id: "chunk-0010",
            page_start: 5,
            page_end: 5,
            chunk_text: "weather field digest",
        },
    );
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
            collection: Some("Phase 9 sqlite sufficiency fixtures".to_owned()),
            record_group: None,
            description: Some("Deterministic SQLite sufficiency eval fixture".to_owned()),
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

fn hit_ids(query_report: &crate::index::LocalSearchQueryReport) -> Vec<&str> {
    query_report
        .hits
        .iter()
        .map(|hit| hit.id.document_key.as_str())
        .collect()
}
