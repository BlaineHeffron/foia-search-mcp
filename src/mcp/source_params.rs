use std::borrow::Cow;

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;

use crate::mcp::sources::VALID_SOURCES;

macro_rules! source_name_type {
    ($name:ident, $description_prefix:literal) => {
        #[derive(Debug, Deserialize)]
        #[serde(transparent)]
        pub(crate) struct $name(String);

        impl $name {
            pub(crate) fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl JsonSchema for $name {
            fn inline_schema() -> bool {
                true
            }

            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
                source_name_schema(&format!(
                    "{}: {}",
                    $description_prefix,
                    allowed_source_list()
                ))
            }
        }
    };
}

source_name_type!(SearchSourceName, "Single source to search");
source_name_type!(SourceRecordSourceName, "Source adapter name");
source_name_type!(LocalSourceFilter, "Optional source filter");

impl LocalSourceFilter {
    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

fn source_name_schema(description: &str) -> Schema {
    json_schema!({
        "type": "string",
        "description": description
    })
}

fn allowed_source_list() -> String {
    match VALID_SOURCES {
        [] => String::new(),
        [single] => (*single).to_owned(),
        [head @ .., last] => format!("{} or {last}", head.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_parameter_schema_descriptions_list_all_valid_sources() {
        let schemas = [
            serde_json::to_string(&schemars::schema_for!(SearchSourceName))
                .expect("search source schema should serialize"),
            serde_json::to_string(&schemars::schema_for!(SourceRecordSourceName))
                .expect("source record schema should serialize"),
            serde_json::to_string(&schemars::schema_for!(LocalSourceFilter))
                .expect("local source filter schema should serialize"),
        ];

        for schema in schemas {
            for source in VALID_SOURCES {
                assert!(
                    schema.contains(source),
                    "source schema description should mention {source}"
                );
            }
        }
    }

    #[test]
    fn source_parameter_schema_descriptions_use_central_source_list() {
        let source_list = allowed_source_list();

        for schema in [
            schemars::schema_for!(SearchSourceName),
            schemars::schema_for!(SourceRecordSourceName),
            schemars::schema_for!(LocalSourceFilter),
        ] {
            let schema_json =
                serde_json::to_string(&schema).expect("source schema should serialize");
            assert!(schema_json.contains(&source_list));
        }
    }
}
