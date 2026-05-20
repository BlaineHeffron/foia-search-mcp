use crate::store::StoreError;

#[derive(Clone, Debug)]
pub struct SearchQuery {
    pub query: String,
    pub source: Option<String>,
    pub limit: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub document_key: crate::store::DocumentKey,
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

pub trait LocalSearchBackend {
    fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, StoreError>;
}

pub struct LocalSearchIndex<B> {
    backend: B,
}

impl<B> LocalSearchIndex<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }
}

impl<B> LocalSearchIndex<B>
where
    B: LocalSearchBackend,
{
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, StoreError> {
        self.backend.search(query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::DocumentKey;

    struct FakeBackend {
        hits: Vec<SearchHit>,
    }

    impl LocalSearchBackend for FakeBackend {
        fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, StoreError> {
            assert_eq!(query.query, "airlift");
            assert_eq!(query.source.as_deref(), Some("cia"));
            assert_eq!(query.limit, 5);
            Ok(self.hits.clone())
        }
    }

    #[test]
    fn local_search_index_delegates_to_backend() {
        let expected = SearchHit {
            document_key: DocumentKey::new("doc_cia_001").expect("safe key"),
            chunk_id: "chunk-1".to_owned(),
            source: "cia".to_owned(),
            title: "Berlin brief".to_owned(),
            page_start: 1,
            page_end: 2,
            score: -1.25,
            snippet: "Berlin [airlift] planning".to_owned(),
            metadata_json: "{}".to_owned(),
            citation_note: Some("Cite the official PDF.".to_owned()),
            terms_note: Some("Public source terms.".to_owned()),
        };
        let index = LocalSearchIndex::new(FakeBackend {
            hits: vec![expected.clone()],
        });

        let hits = index
            .search(&SearchQuery {
                query: "airlift".to_owned(),
                source: Some("cia".to_owned()),
                limit: 5,
            })
            .expect("delegate search");

        assert_eq!(hits, vec![expected]);
    }
}
