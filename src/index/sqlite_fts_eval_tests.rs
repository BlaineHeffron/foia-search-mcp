use super::{FtsSearch, SearchQuery};
use crate::store::{ChunkInput, DocumentKey, PageInput, SqliteStore, TextSource, UpsertDocument};

#[test]
fn sqlite_phase9_baseline_keeps_mixed_source_top_k_order_stable() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    seed_fixture(
        &mut store,
        FixtureDoc {
            document_key: "doc_cia_weather_001",
            public_id: "cia:weather-001",
            source: "cia",
            title: "CIA Weather Control Memo",
            metadata_json: r#"{"source_metadata":{"source_warning":"CIA source warning"}}"#,
            citation_note: Some("Cite CIA PDF."),
            terms_note: Some("CIA terms."),
            chunk_id: "chunk-0001",
            page_start: 1,
            page_end: 2,
            chunk_text: "weather modification weather modification cloud seeding field report",
        },
    );
    seed_fixture(
        &mut store,
        FixtureDoc {
            document_key: "doc_nsa_weather_001",
            public_id: "nsa:weather-001",
            source: "nsa",
            title: "NSA Weather Notes",
            metadata_json: r#"{"source_metadata":{"source_warning":"NSA source warning"}}"#,
            citation_note: Some("Cite NSA PDF."),
            terms_note: Some("NSA terms."),
            chunk_id: "chunk-0001",
            page_start: 3,
            page_end: 4,
            chunk_text: "weather modification signal review",
        },
    );
    seed_fixture(
        &mut store,
        FixtureDoc {
            document_key: "doc_dia_weather_001",
            public_id: "dia:weather-001",
            source: "dia",
            title: "DIA Weather Digest",
            metadata_json: r#"{"source_metadata":{"source_warning":"DIA source warning"}}"#,
            citation_note: Some("Cite DIA PDF."),
            terms_note: Some("DIA terms."),
            chunk_id: "chunk-0001",
            page_start: 5,
            page_end: 5,
            chunk_text: "weather field digest",
        },
    );

    let hits = FtsSearch::new(&store)
        .search(&SearchQuery {
            query: "weather modification".to_owned(),
            source: None,
            limit: 2,
        })
        .expect("search mixed-source corpus");

    assert_eq!(hits.len(), 2);
    assert_eq!(
        hit_ids(&hits),
        ["doc_cia_weather_001", "doc_nsa_weather_001"]
    );
    assert!(hits[0].score <= hits[1].score);
    assert_eq!(hits[0].source, "cia");
    assert_eq!(hits[0].title, "CIA Weather Control Memo");
    assert_eq!(hits[0].page_start, 1);
    assert_eq!(hits[0].page_end, 2);
    assert!(hits[0].snippet.contains("[weather]"));
    assert!(hits[0].metadata_json.contains("CIA source warning"));
    assert_eq!(hits[0].citation_note.as_deref(), Some("Cite CIA PDF."));
    assert_eq!(hits[0].terms_note.as_deref(), Some("CIA terms."));
}

#[test]
fn sqlite_phase9_baseline_enforces_source_filter_without_shape_regression() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    seed_fixture(
        &mut store,
        FixtureDoc {
            document_key: "doc_cia_weather_002",
            public_id: "cia:weather-002",
            source: "cia",
            title: "CIA Storm Report",
            metadata_json: r#"{"source_metadata":{"source_warning":"CIA storm warning"}}"#,
            citation_note: Some("Cite CIA storm PDF."),
            terms_note: Some("CIA storm terms."),
            chunk_id: "chunk-0002",
            page_start: 7,
            page_end: 8,
            chunk_text: "weather modification operational summary",
        },
    );
    seed_fixture(
        &mut store,
        FixtureDoc {
            document_key: "doc_nsa_weather_002",
            public_id: "nsa:weather-002",
            source: "nsa",
            title: "NSA Storm Report",
            metadata_json: r#"{"source_metadata":{"source_warning":"NSA storm warning"}}"#,
            citation_note: Some("Cite NSA storm PDF."),
            terms_note: Some("NSA storm terms."),
            chunk_id: "chunk-0002",
            page_start: 2,
            page_end: 2,
            chunk_text: "weather modification operational summary",
        },
    );

    let hits = FtsSearch::new(&store)
        .search(&SearchQuery {
            query: "weather modification".to_owned(),
            source: Some("cia".to_owned()),
            limit: 10,
        })
        .expect("search with source filter");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].document_key.as_str(), "doc_cia_weather_002");
    assert_eq!(hits[0].chunk_id, "chunk-0002");
    assert_eq!(hits[0].source, "cia");
    assert_eq!(hits[0].title, "CIA Storm Report");
    assert_eq!(hits[0].page_start, 7);
    assert_eq!(hits[0].page_end, 8);
    assert!(hits[0].snippet.contains("[weather]"));
    assert!(hits[0].metadata_json.contains("CIA storm warning"));
    assert_eq!(
        hits[0].citation_note.as_deref(),
        Some("Cite CIA storm PDF.")
    );
    assert_eq!(hits[0].terms_note.as_deref(), Some("CIA storm terms."));
}

#[test]
fn sqlite_phase9_baseline_returns_empty_hits_for_empty_or_sparse_corpora() {
    let empty_store = SqliteStore::open_memory().expect("open empty in-memory store");
    let empty_hits = FtsSearch::new(&empty_store)
        .search(&SearchQuery {
            query: "weather".to_owned(),
            source: None,
            limit: 10,
        })
        .expect("search empty corpus");
    assert!(empty_hits.is_empty());

    let mut sparse_store = SqliteStore::open_memory().expect("open sparse in-memory store");
    seed_fixture(
        &mut sparse_store,
        FixtureDoc {
            document_key: "doc_cia_sparse_001",
            public_id: "cia:sparse-001",
            source: "cia",
            title: "CIA Sparse Corpus Note",
            metadata_json: r#"{"source_metadata":{"source_warning":"Sparse corpus warning"}}"#,
            citation_note: Some("Cite sparse CIA note."),
            terms_note: Some("Sparse CIA terms."),
            chunk_id: "chunk-0001",
            page_start: 1,
            page_end: 1,
            chunk_text: "archival logistics memo",
        },
    );

    let sparse_hits = FtsSearch::new(&sparse_store)
        .search(&SearchQuery {
            query: "weather modification".to_owned(),
            source: Some("cia".to_owned()),
            limit: 10,
        })
        .expect("search sparse corpus");
    assert!(sparse_hits.is_empty());
}

#[test]
fn sqlite_phase9_baseline_preserves_result_shape_fields_relevant_to_evals() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    seed_fixture(
        &mut store,
        FixtureDoc {
            document_key: "doc_doj_epstein_eval_001",
            public_id: "doj_epstein:eval-001",
            source: "doj_epstein",
            title: "DOJ Epstein Eval Fixture",
            metadata_json: r#"{"source_metadata":{"source_warning":"Sensitive DOJ warning","collection":"Epstein Library"}}"#,
            citation_note: Some("Cite official DOJ page/PDF URL."),
            terms_note: Some("Sensitive DOJ Epstein Library content."),
            chunk_id: "chunk-0099",
            page_start: 11,
            page_end: 13,
            chunk_text: "epstein disclosure fixture with sensitive warning context",
        },
    );

    let hits = FtsSearch::new(&store)
        .search(&SearchQuery {
            query: "sensitive warning".to_owned(),
            source: Some("doj_epstein".to_owned()),
            limit: 5,
        })
        .expect("search parity fixture");

    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.document_key.as_str(), "doc_doj_epstein_eval_001");
    assert_eq!(hit.chunk_id, "chunk-0099");
    assert_eq!(hit.source, "doj_epstein");
    assert_eq!(hit.title, "DOJ Epstein Eval Fixture");
    assert_eq!((hit.page_start, hit.page_end), (11, 13));
    assert!(hit.snippet.contains("[sensitive]") || hit.snippet.contains("[warning]"));
    assert!(hit.metadata_json.contains("Sensitive DOJ warning"));
    assert_eq!(
        hit.citation_note.as_deref(),
        Some("Cite official DOJ page/PDF URL.")
    );
    assert_eq!(
        hit.terms_note.as_deref(),
        Some("Sensitive DOJ Epstein Library content.")
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
            collection: Some("Phase 9 eval fixtures".to_owned()),
            record_group: None,
            description: Some("Deterministic SQLite local-search baseline fixture".to_owned()),
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

fn hit_ids(hits: &[crate::index::SearchHit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.document_key.as_str()).collect()
}
