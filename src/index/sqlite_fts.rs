use crate::store::{DocumentKey, SqliteStore, StoreError};
use rusqlite::params;

#[derive(Clone, Debug)]
pub struct SearchQuery {
    pub query: String,
    pub source: Option<String>,
    pub limit: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub document_key: DocumentKey,
    pub chunk_id: String,
    pub source: String,
    pub title: String,
    pub page_start: i64,
    pub page_end: i64,
    pub score: f64,
    pub snippet: String,
    pub metadata_json: String,
    pub citation_note: Option<String>,
    pub terms_note: Option<String>,
}

pub struct FtsSearch<'a> {
    store: &'a SqliteStore,
}

impl<'a> FtsSearch<'a> {
    pub fn new(store: &'a SqliteStore) -> Self {
        Self { store }
    }

    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, StoreError> {
        let limit = query.limit.clamp(1, 100);
        match &query.source {
            Some(source) => self.search_with_source(&query.query, source, limit),
            None => self.search_all_sources(&query.query, limit),
        }
    }

    fn search_all_sources(&self, query: &str, limit: i64) -> Result<Vec<SearchHit>, StoreError> {
        let mut stmt = self.store.connection().prepare(
            "
            SELECT f.document_key, f.chunk_id, f.source, f.title, f.page_start, f.page_end,
                bm25(chunk_fts) AS score,
                snippet(chunk_fts, 4, '[', ']', '...', 32) AS snippet,
                d.metadata_json, d.citation_note, d.terms_note
            FROM chunk_fts f
            JOIN documents d ON d.document_key = f.document_key
            WHERE chunk_fts MATCH ?1
            ORDER BY score
            LIMIT ?2
            ",
        )?;
        let rows = stmt.query_map(params![query, limit], read_hit)?;
        collect_hits(rows)
    }

    fn search_with_source(
        &self,
        query: &str,
        source: &str,
        limit: i64,
    ) -> Result<Vec<SearchHit>, StoreError> {
        let mut stmt = self.store.connection().prepare(
            "
            SELECT f.document_key, f.chunk_id, f.source, f.title, f.page_start, f.page_end,
                bm25(chunk_fts) AS score,
                snippet(chunk_fts, 4, '[', ']', '...', 32) AS snippet,
                d.metadata_json, d.citation_note, d.terms_note
            FROM chunk_fts f
            JOIN documents d ON d.document_key = f.document_key
            WHERE chunk_fts MATCH ?1 AND f.source = ?2
            ORDER BY score
            LIMIT ?3
            ",
        )?;
        let rows = stmt.query_map(params![query, source, limit], read_hit)?;
        collect_hits(rows)
    }
}

fn read_hit(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchHit> {
    let key: String = row.get(0)?;
    Ok(SearchHit {
        document_key: DocumentKey::new(key).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
        })?,
        chunk_id: row.get(1)?,
        source: row.get(2)?,
        title: row.get(3)?,
        page_start: row.get(4)?,
        page_end: row.get(5)?,
        score: row.get(6)?,
        snippet: row.get(7)?,
        metadata_json: row.get(8)?,
        citation_note: row.get(9)?,
        terms_note: row.get(10)?,
    })
}

fn collect_hits<F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Vec<SearchHit>, StoreError>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<SearchHit>,
{
    let mut hits = Vec::new();
    for hit in rows {
        hits.push(hit?);
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{ChunkInput, PageInput, TextSource, UpsertDocument};

    #[test]
    fn search_returns_page_aware_chunk_hits() {
        let mut store = SqliteStore::open_memory().expect("open in-memory store");
        let key = DocumentKey::new("doc_cia_001").expect("safe key");
        store
            .upsert_document(&UpsertDocument {
                public_id: "cia:CREST-001".to_owned(),
                document_key: key.clone(),
                source: "cia".to_owned(),
                source_id: "CREST-001".to_owned(),
                title: "Berlin brief".to_owned(),
                date: None,
                collection: None,
                record_group: None,
                description: None,
                origin_url: None,
                document_url: None,
                pdf_url: None,
                metadata_json: "{}".to_owned(),
                citation_note: None,
                terms_note: None,
            })
            .expect("insert document");
        store
            .replace_pages_and_chunks(
                &key,
                &[PageInput {
                    document_key: key.clone(),
                    page_number: 1,
                    text: "Berlin airlift planning".to_owned(),
                    text_source: TextSource::EmbeddedPdfText,
                    quality_score: Some(0.9),
                    warnings_json: "[]".to_owned(),
                }],
                &[ChunkInput {
                    document_key: key.clone(),
                    chunk_id: "c1".to_owned(),
                    page_start: 1,
                    page_end: 1,
                    text: "Berlin airlift planning memo".to_owned(),
                    token_estimate: Some(4),
                    metadata_json: "{}".to_owned(),
                }],
            )
            .expect("replace pages and chunks");

        let hits = FtsSearch::new(&store)
            .search(&SearchQuery {
                query: "airlift".to_owned(),
                source: Some("cia".to_owned()),
                limit: 10,
            })
            .expect("search fts");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].document_key, key);
        assert_eq!(hits[0].page_start, 1);
        assert_eq!(hits[0].page_end, 1);
    }

    #[test]
    fn search_hit_carries_persisted_document_notes_and_warning_metadata() {
        let mut store = SqliteStore::open_memory().expect("open in-memory store");
        let key = DocumentKey::new("doc_doj_epstein_001").expect("safe key");
        store
            .upsert_document(&UpsertDocument {
                public_id: "doj_epstein:data-set-1-files".to_owned(),
                document_key: key.clone(),
                source: "doj_epstein".to_owned(),
                source_id: "data-set-1-files".to_owned(),
                title: "DOJ Epstein Library fixture".to_owned(),
                date: None,
                collection: Some("DOJ Epstein Library".to_owned()),
                record_group: None,
                description: None,
                origin_url: None,
                document_url: None,
                pdf_url: None,
                metadata_json: r#"{"source_metadata":{"source_warning":"DOJ privacy warning"}}"#
                    .to_owned(),
                citation_note: Some("Cite official DOJ page/PDF URL.".to_owned()),
                terms_note: Some("Sensitive DOJ Epstein Library content.".to_owned()),
            })
            .expect("insert document");
        store
            .replace_pages_and_chunks(
                &key,
                &[PageInput {
                    document_key: key.clone(),
                    page_number: 1,
                    text: "Epstein fixture text".to_owned(),
                    text_source: TextSource::EmbeddedPdfText,
                    quality_score: Some(0.9),
                    warnings_json: "[]".to_owned(),
                }],
                &[ChunkInput {
                    document_key: key.clone(),
                    chunk_id: "c1".to_owned(),
                    page_start: 1,
                    page_end: 1,
                    text: "Epstein fixture text".to_owned(),
                    token_estimate: Some(3),
                    metadata_json: "{}".to_owned(),
                }],
            )
            .expect("replace pages and chunks");

        let hits = FtsSearch::new(&store)
            .search(&SearchQuery {
                query: "fixture".to_owned(),
                source: Some("doj_epstein".to_owned()),
                limit: 10,
            })
            .expect("search fts");

        assert_eq!(hits.len(), 1);
        assert!(hits[0].metadata_json.contains("DOJ privacy warning"));
        assert_eq!(
            hits[0].citation_note.as_deref(),
            Some("Cite official DOJ page/PDF URL.")
        );
        assert_eq!(
            hits[0].terms_note.as_deref(),
            Some("Sensitive DOJ Epstein Library content.")
        );
    }
}
