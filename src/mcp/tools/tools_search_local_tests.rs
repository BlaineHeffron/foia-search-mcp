use crate::store::{ChunkInput, PageInput, TextSource};
use serde_json::{json, Value};

#[tokio::test]
async fn search_local_documents_returns_object_shape_for_non_empty_results() {
    let temp = tempfile::tempdir().expect("tempdir");
    seed_local_search_fixture(temp.path());
    let server = FoiaSearchServer::from_parts(
        Arc::new(search_local_test_config(temp.path())),
        Arc::new(Vec::new()),
    );

    let response = search_local_response_json(&server, "weather modification", Some("cia"), 5)
        .await
        .expect("search response");

    let hits = response["hits"].as_array().expect("hits array");
    assert_eq!(hits.len(), 1);
    assert!(
        response["next_actions"]
            .as_array()
            .expect("next_actions array")
            .is_empty()
    );
    assert_eq!(hits[0]["document_key"], "doc_cia_local_search_fixture");
    assert_eq!(hits[0]["chunk_id"], "chunk-0001");
    assert_eq!(hits[0]["source"], "cia");
    assert_eq!(hits[0]["title"], "CIA Local Search Fixture");
    assert_eq!(hits[0]["page_start"], 1);
    assert_eq!(hits[0]["page_end"], 2);
    assert_eq!(
        hits[0]["citation_note"],
        Value::String("Cite CIA local fixture PDF.".to_owned())
    );
    assert_eq!(
        hits[0]["terms_note"],
        Value::String("CIA local fixture terms.".to_owned())
    );
    assert_eq!(
        hits[0]["source_warning"],
        Value::String("CIA fixture warning".to_owned())
    );
    assert_eq!(
        hits[0]["warnings"],
        Value::Array(vec![Value::String("CIA fixture warning".to_owned())])
    );
}

#[tokio::test]
async fn search_local_documents_returns_actionable_guidance_for_empty_corpus() {
    let temp = tempfile::tempdir().expect("tempdir");
    let server = FoiaSearchServer::from_parts(
        Arc::new(search_local_test_config(temp.path())),
        Arc::new(Vec::new()),
    );

    let response = search_local_response_json(&server, "weather modification", None, 10)
        .await
        .expect("search response");

    assert!(
        response["hits"]
            .as_array()
            .expect("hits array")
            .is_empty()
    );
    let next_actions = response["next_actions"]
        .as_array()
        .expect("next_actions array");
    assert_eq!(next_actions.len(), 1);
    assert_eq!(
        next_actions[0],
        Value::String(
            "No local hits. Ingest local documents first or broaden query/source constraints."
                .to_owned()
        )
    );
}

#[tokio::test]
async fn search_local_documents_returns_source_specific_guidance_for_filter_miss() {
    let temp = tempfile::tempdir().expect("tempdir");
    seed_local_search_fixture(temp.path());
    let server = FoiaSearchServer::from_parts(
        Arc::new(search_local_test_config(temp.path())),
        Arc::new(Vec::new()),
    );

    let response = search_local_response_json(&server, "weather modification", Some("nsa"), 10)
        .await
        .expect("search response");

    assert!(
        response["hits"]
            .as_array()
            .expect("hits array")
            .is_empty()
    );
    let next_actions = response["next_actions"]
        .as_array()
        .expect("next_actions array");
    assert_eq!(next_actions.len(), 1);
    assert_eq!(
        next_actions[0],
        Value::String(
            "No local hits for source 'nsa'. Ingest additional local documents for that source or broaden query terms."
                .to_owned()
        )
    );
}

async fn search_local_response_json(
    server: &FoiaSearchServer,
    query: &str,
    source: Option<&str>,
    limit: u32,
) -> Result<Value, Box<dyn std::error::Error>> {
    let params: SearchLocalDocumentsParams = serde_json::from_value(json!({
        "query": query,
        "source": source,
        "limit": limit
    }))?;
    let response = server.search_local_documents(Parameters(params)).await?;
    let payload = response
        .content
        .first()
        .and_then(|content| content.as_text())
        .map(|text| text.text.as_str())
        .ok_or("search_local_documents should return text content")?;
    let parsed = serde_json::from_str(payload)?;
    Ok(parsed)
}

fn search_local_test_config(data_dir: &std::path::Path) -> Config {
    Config {
        data_dir: data_dir.to_owned(),
        nara_api_key: None,
        nara_api_base_url: "https://catalog.archives.gov/api/v2".to_owned(),
        ocr_fallback_policy: OcrFallbackPolicy::off(),
        ocr_backend: OcrBackendConfig::default(),
    }
}

fn seed_local_search_fixture(data_dir: &std::path::Path) {
    let db_dir = data_dir.join("db");
    std::fs::create_dir_all(&db_dir).expect("create db dir");
    let mut store = SqliteStore::open(db_dir.join("foia.sqlite")).expect("open store");
    let document_key = DocumentKey::new("doc_cia_local_search_fixture").expect("safe key");

    store
        .upsert_document(&UpsertDocument {
            public_id: "cia:local-search-fixture".to_owned(),
            document_key: document_key.clone(),
            source: "cia".to_owned(),
            source_id: "local-search-fixture".to_owned(),
            title: "CIA Local Search Fixture".to_owned(),
            date: Some("1961-01-01".to_owned()),
            collection: Some("CREST".to_owned()),
            record_group: None,
            description: Some("Deterministic local-search MCP fixture".to_owned()),
            origin_url: Some(
                "https://www.cia.gov/readingroom/document/local-search-fixture".to_owned(),
            ),
            document_url: Some(
                "https://www.cia.gov/readingroom/document/local-search-fixture".to_owned(),
            ),
            pdf_url: Some(
                "https://www.cia.gov/readingroom/docs/local-search-fixture.pdf".to_owned(),
            ),
            metadata_json: r#"{"source_metadata":{"source_warning":"CIA fixture warning"}}"#
                .to_owned(),
            citation_note: Some("Cite CIA local fixture PDF.".to_owned()),
            terms_note: Some("CIA local fixture terms.".to_owned()),
        })
        .expect("seed local search document");
    store
        .replace_pages_and_chunks(
            &document_key,
            &[PageInput {
                document_key: document_key.clone(),
                page_number: 1,
                text: "weather modification fixture page".to_owned(),
                text_source: TextSource::EmbeddedPdfText,
                quality_score: Some(0.9),
                warnings_json: "[]".to_owned(),
            }],
            &[ChunkInput {
                document_key: document_key.clone(),
                chunk_id: "chunk-0001".to_owned(),
                page_start: 1,
                page_end: 2,
                text: "weather modification fixture chunk body".to_owned(),
                token_estimate: Some(5),
                metadata_json: "{}".to_owned(),
            }],
        )
        .expect("seed local search chunks");
}
