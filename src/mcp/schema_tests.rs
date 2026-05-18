use std::collections::BTreeMap;

use rmcp::model::Tool;
use serde_json::Value;

use crate::mcp::{sources::VALID_SOURCES, tools::FoiaSearchServer};

const EXPECTED_TOOL_NAMES: &[&str] = &[
    "apply_derived_artifact_repairs",
    "apply_sqlite_fts_repairs",
    "get_document",
    "get_document_text",
    "get_ingestion_job",
    "get_source_record",
    "ingest_document",
    "list_sources",
    "plan_derived_artifact_repairs",
    "plan_sqlite_fts_repairs",
    "refresh_document",
    "report_derived_artifact_drift",
    "report_sqlite_fts_drift",
    "search_local_documents",
    "search_source",
];

#[test]
fn all_expected_mcp_tools_are_registered_with_metadata() {
    let tools = registered_tools_by_name();
    let actual_names = tools.keys().map(String::as_str).collect::<Vec<_>>();

    assert_eq!(actual_names, EXPECTED_TOOL_NAMES);

    for (name, tool) in tools {
        assert!(
            tool.description
                .as_ref()
                .is_some_and(|description| !description.trim().is_empty()),
            "{name} should have a non-empty description"
        );
        assert!(
            !tool.input_schema.is_empty(),
            "{name} should have a non-empty input schema"
        );
    }
}

#[test]
fn source_bearing_tool_schemas_include_the_central_source_list() {
    let tools = registered_tools_by_name();
    let expected_source_list = format_valid_sources();

    for (tool_name, field_name) in [
        ("search_source", "source"),
        ("get_source_record", "source"),
        ("search_local_documents", "source"),
    ] {
        let schema = schema_value(
            tools
                .get(tool_name)
                .unwrap_or_else(|| panic!("{tool_name} should be registered")),
        );
        let description = property_description(&schema, field_name)
            .unwrap_or_else(|| panic!("{tool_name}.{field_name} should have a description"));

        assert!(
            description.contains(&expected_source_list),
            "{tool_name}.{field_name} should include central source list; got {description:?}"
        );
        for source in VALID_SOURCES {
            assert!(
                description.contains(source),
                "{tool_name}.{field_name} should mention source {source}"
            );
        }
    }
}

#[test]
fn repair_apply_tool_schemas_expose_confirmation_guidance() {
    let tools = registered_tools_by_name();

    assert_property_description_contains(
        &tools,
        "apply_derived_artifact_repairs",
        "confirmation",
        "apply derived artifact repairs for <document_id>",
    );
    assert_property_description_contains(
        &tools,
        "apply_sqlite_fts_repairs",
        "confirmation",
        "apply sqlite fts repairs",
    );
}

#[test]
fn repair_apply_tool_descriptions_name_confirmation_and_manual_review_behavior() {
    let tools = registered_tools_by_name();

    let derived = description_for(&tools, "apply_derived_artifact_repairs");
    assert!(derived.contains("requires explicit confirmation"));
    assert!(derived.contains("apply derived artifact repairs for <document_id>"));

    let fts = description_for(&tools, "apply_sqlite_fts_repairs");
    assert!(fts.contains("requires explicit confirmation"));
    assert!(fts.contains("apply sqlite fts repairs"));
    assert!(fts.contains("manual review"));
}

fn registered_tools_by_name() -> BTreeMap<String, Tool> {
    FoiaSearchServer::tool_router()
        .list_all()
        .into_iter()
        .map(|tool| (tool.name.to_string(), tool))
        .collect()
}

fn schema_value(tool: &Tool) -> Value {
    serde_json::to_value(tool.input_schema.as_ref()).unwrap_or_else(|error| {
        panic!("{} input schema should serialize: {error}", tool.name);
    })
}

fn property_description(schema: &Value, property_name: &str) -> Option<String> {
    schema
        .get("properties")?
        .get(property_name)?
        .get("description")?
        .as_str()
        .map(str::to_owned)
}

fn assert_property_description_contains(
    tools: &BTreeMap<String, Tool>,
    tool_name: &str,
    property_name: &str,
    expected: &str,
) {
    let schema = schema_value(
        tools
            .get(tool_name)
            .unwrap_or_else(|| panic!("{tool_name} should be registered")),
    );
    let description = property_description(&schema, property_name)
        .unwrap_or_else(|| panic!("{tool_name}.{property_name} should have a description"));

    assert!(
        description.contains(expected),
        "{tool_name}.{property_name} should mention {expected:?}; got {description:?}"
    );
}

fn description_for(tools: &BTreeMap<String, Tool>, tool_name: &str) -> String {
    tools
        .get(tool_name)
        .unwrap_or_else(|| panic!("{tool_name} should be registered"))
        .description
        .as_ref()
        .unwrap_or_else(|| panic!("{tool_name} should have a description"))
        .to_string()
}

fn format_valid_sources() -> String {
    match VALID_SOURCES {
        [] => String::new(),
        [single] => (*single).to_owned(),
        [head @ .., last] => format!("{} or {last}", head.join(", ")),
    }
}
