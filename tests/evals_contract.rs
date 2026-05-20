use std::collections::BTreeSet;

use foia_search::sources::registry::SOURCE_NAMES;

const EVALS_XML: &str = include_str!("../evals.xml");

#[derive(Debug)]
struct EvalCase {
    id: String,
    question: String,
    expected: String,
}

#[test]
fn evals_xml_has_readable_structure_unique_ids_and_current_wording() {
    let evals = parse_evals(EVALS_XML);
    assert!(evals.len() >= 10, "expected at least 10 evals");

    let mut ids = BTreeSet::new();
    for eval in &evals {
        assert!(!eval.id.trim().is_empty(), "eval id must be non-empty");
        assert!(
            ids.insert(eval.id.as_str()),
            "duplicate eval id {}",
            eval.id
        );
        assert!(
            !eval.question.trim().is_empty(),
            "eval {} question must be non-empty",
            eval.id
        );
        assert!(
            !eval.expected.trim().is_empty(),
            "eval {} expected answer must be non-empty",
            eval.id
        );
    }

    let all_text = EVALS_XML.to_ascii_lowercase();
    for stale in [
        "not fully wired",
        "not yet wired",
        "worker is unwired",
        "manual_tracer",
        "manual tracer",
        "disabled/manual",
    ] {
        assert!(
            !all_text.contains(stale),
            "evals.xml contains stale implementation wording: {stale}"
        );
    }
}

#[test]
fn every_registered_source_has_eval_coverage() {
    let evals = parse_evals(EVALS_XML);

    for source in SOURCE_NAMES {
        assert!(
            evals.iter().any(|eval| mentions_source(eval, source)),
            "evals.xml should include at least one model-facing task for source {source}"
        );
    }
}

#[test]
fn repair_direct_ingestion_and_sensitive_source_guardrails_are_covered() {
    let all_text = EVALS_XML.to_ascii_lowercase();

    for required in [
        "report_derived_artifact_drift",
        "plan_derived_artifact_repairs",
        "apply_derived_artifact_repairs",
        "report_sqlite_fts_drift",
        "plan_sqlite_fts_repairs",
        "apply_sqlite_fts_repairs",
        "startup",
        "worker recovery",
        "auto-repair",
        "operator-facing",
        "direct url",
        "local-file ingestion",
        "disabled by default",
        "doj_epstein",
        "privacy",
        "victim-identification",
    ] {
        assert!(
            all_text.contains(required),
            "evals.xml should cover guardrail phrase/tool: {required}"
        );
    }
}

#[test]
fn repair_eval_cases_pin_explicit_report_plan_apply_contracts() {
    let evals = parse_evals(EVALS_XML);

    let derived_apply = eval_by_id(&evals, "25");
    for required in [
        "report_derived_artifact_drift",
        "plan_derived_artifact_repairs",
        "apply_derived_artifact_repairs",
        "do not assume automatic repair",
        "apply derived artifact repairs for cia:CREST-weather",
        "manual_review",
        "orphaned artifacts",
    ] {
        assert!(
            derived_apply.expected.contains(required),
            "eval 25 expected text should include {required}"
        );
    }

    let derived_bad_confirmation = eval_by_id(&evals, "26");
    for required in [
        "invalid_params",
        "exact string",
        "apply derived artifact repairs for cia:CREST-weather",
        "manual-review only",
    ] {
        assert!(
            derived_bad_confirmation.expected.contains(required),
            "eval 26 expected text should include {required}"
        );
    }

    let fts_apply = eval_by_id(&evals, "28");
    for required in [
        "report_sqlite_fts_drift",
        "plan_sqlite_fts_repairs",
        "apply_sqlite_fts_repairs",
        "do not assume automatic repair",
        "apply sqlite fts repairs",
        "manual_review",
        "orphaned chunk_fts rows",
    ] {
        assert!(
            fts_apply.expected.contains(required),
            "eval 28 expected text should include {required}"
        );
    }

    let fts_bad_confirmation = eval_by_id(&evals, "29");
    for required in [
        "invalid_params",
        "exact string",
        "apply sqlite fts repairs",
        "manual-review only",
    ] {
        assert!(
            fts_bad_confirmation.expected.contains(required),
            "eval 29 expected text should include {required}"
        );
    }

    let boundary = eval_by_id(&evals, "repair-startup-worker-boundary");
    let boundary_text = format!(
        "{} {}",
        boundary.question.to_ascii_lowercase(),
        boundary.expected.to_ascii_lowercase()
    );
    for required in [
        "startup/runtime/worker recovery",
        "derived artifacts",
        "sqlite fts drift",
        "must not auto-repair",
        "report/plan/apply tools",
        "exact confirmation string",
        "manual-review",
    ] {
        assert!(
            boundary_text.contains(required),
            "repair-startup-worker-boundary question/expected text should include {required}"
        );
    }
}

fn parse_evals(xml: &str) -> Vec<EvalCase> {
    let mut lines = xml.lines().map(str::trim).filter(|line| !line.is_empty());

    assert_eq!(
        lines.next(),
        Some("<evals>"),
        "evals.xml should start with <evals>"
    );

    let mut evals = Vec::new();
    let mut current: Option<EvalCase> = None;
    let mut saw_close_root = false;

    for line in lines {
        if line == "</evals>" {
            assert!(current.is_none(), "root closed while an eval was open");
            saw_close_root = true;
            continue;
        }

        if saw_close_root {
            panic!("content appears after </evals>: {line}");
        }

        if let Some(id) = parse_eval_id(line) {
            assert!(current.is_none(), "nested eval start found at id {id}");
            current = Some(EvalCase {
                id,
                question: String::new(),
                expected: String::new(),
            });
            continue;
        }

        if line == "</eval>" {
            let eval = current
                .take()
                .unwrap_or_else(|| panic!("closing eval without an open eval"));
            assert!(
                !eval.question.is_empty() && !eval.expected.is_empty(),
                "eval {} must include question and expected",
                eval.id
            );
            evals.push(eval);
            continue;
        }

        let eval = current
            .as_mut()
            .unwrap_or_else(|| panic!("content outside eval element: {line}"));

        if let Some(question) = line
            .strip_prefix("<question>")
            .and_then(|value| value.strip_suffix("</question>"))
        {
            assert!(
                eval.question.is_empty(),
                "duplicate question in {}",
                eval.id
            );
            eval.question = question.to_owned();
        } else if let Some(expected) = line
            .strip_prefix("<expected>")
            .and_then(|value| value.strip_suffix("</expected>"))
        {
            assert!(
                eval.expected.is_empty(),
                "duplicate expected in {}",
                eval.id
            );
            eval.expected = expected.to_owned();
        } else {
            panic!("unexpected evals.xml line shape: {line}");
        }
    }

    assert!(saw_close_root, "evals.xml should close with </evals>");
    evals
}

fn parse_eval_id(line: &str) -> Option<String> {
    let raw_id = line.strip_prefix("<eval id=\"")?.strip_suffix("\">")?;

    Some(raw_id.to_owned())
}

fn eval_by_id<'a>(evals: &'a [EvalCase], id: &str) -> &'a EvalCase {
    evals
        .iter()
        .find(|eval| eval.id == id)
        .unwrap_or_else(|| panic!("missing eval id {id}"))
}

fn mentions_source(eval: &EvalCase, source: &str) -> bool {
    let text = format!(
        "{} {} {}",
        eval.id.to_ascii_lowercase(),
        eval.question.to_ascii_lowercase(),
        eval.expected.to_ascii_lowercase()
    );

    contains_token(&text, source)
}

fn contains_token(text: &str, token: &str) -> bool {
    text.match_indices(token).any(|(start, _)| {
        let end = start + token.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();

        !before.is_some_and(is_source_token_char) && !after.is_some_and(is_source_token_char)
    })
}

fn is_source_token_char(ch: char) -> bool {
    ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'
}
