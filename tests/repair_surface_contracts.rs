const STARTUP_AND_WORKER_SURFACES: &[(&str, &str)] = &[
    ("src/main.rs", include_str!("../src/main.rs")),
    ("src/runtime.rs", include_str!("../src/runtime.rs")),
    (
        "src/ingest/worker.rs",
        include_str!("../src/ingest/worker.rs"),
    ),
    (
        "src/ingest/executor.rs",
        include_str!("../src/ingest/executor.rs"),
    ),
];

const REPAIR_API_TOKENS: &[&str] = &[
    "report_derived_artifact_drift",
    "plan_derived_artifact_repairs",
    "apply_derived_artifact_repairs",
    "reconcile_derived_artifacts_for_document",
    "report_sqlite_fts_drift",
    "plan_sqlite_fts_repairs",
    "apply_sqlite_fts_repairs",
    "reconcile_sqlite_fts_index",
];

#[test]
fn startup_runtime_and_worker_do_not_reference_repair_surfaces() {
    for (path, source) in STARTUP_AND_WORKER_SURFACES {
        for token in REPAIR_API_TOKENS {
            assert!(
                !source.contains(token),
                "{path} must not reference repair API token {token}; repair belongs behind explicit operator MCP report/plan/apply tools"
            );
        }
    }
}

#[test]
fn repair_references_stay_on_operator_facing_surfaces() {
    let tools = include_str!("../src/mcp/tools.rs");
    let derived = include_str!("../src/mcp/repair.rs");
    let fts = include_str!("../src/mcp/fts_repair.rs");

    for token in [
        "report_derived_artifact_drift",
        "plan_derived_artifact_repairs",
        "apply_derived_artifact_repairs",
    ] {
        assert!(
            tools.contains(token) || derived.contains(token),
            "derived-artifact repair token {token} should remain reachable through explicit MCP repair surfaces"
        );
    }

    for token in [
        "report_sqlite_fts_drift",
        "plan_sqlite_fts_repairs",
        "apply_sqlite_fts_repairs",
    ] {
        assert!(
            tools.contains(token) || fts.contains(token),
            "SQLite FTS repair token {token} should remain reachable through explicit MCP repair surfaces"
        );
    }
}
