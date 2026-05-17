#!/usr/bin/env bash
set -euo pipefail

mode="${1:-full}"

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

failures=0

report_failure() {
    printf 'ai-dev-gates: %s\n' "$1" >&2
    failures=$((failures + 1))
}

tracked_files() {
    git ls-files "$@"
}

staged_files() {
    git diff --cached --name-only --diff-filter=ACMR "$@"
}

if [[ "$mode" == "--pre-commit" ]]; then
    mapfile -t rust_files < <(staged_files '*.rs')
    mapfile -t changed_files < <(staged_files)
else
    mapfile -t rust_files < <(tracked_files '*.rs')
    mapfile -t changed_files < <(tracked_files)
fi

check_file_bloat() {
    local file line_count allowed
    declare -A frozen_limits=(
        ["src/mcp/tools.rs"]=779
        ["src/sources/cia.rs"]=763
        ["src/store/sqlite.rs"]=747
        ["src/sources/nara.rs"]=648
    )

    for file in "${rust_files[@]}"; do
        [[ -f "$file" ]] || continue
        line_count="$(wc -l < "$file" | tr -d ' ')"
        allowed="${frozen_limits[$file]:-600}"

        if [[ "$line_count" -gt "$allowed" ]]; then
            if [[ -n "${frozen_limits[$file]:-}" ]]; then
                report_failure "$file is already oversized and may not grow past $allowed lines; split new behavior into a submodule."
            else
                report_failure "$file has $line_count lines; keep Rust modules at or below 600 lines unless this script gets a reviewed exception."
            fi
        fi
    done
}

check_new_file_size() {
    local file line_count
    if [[ "$mode" != "--pre-commit" ]]; then
        return
    fi

    while IFS= read -r file; do
        [[ -f "$file" ]] || continue
        line_count="$(wc -l < "$file" | tr -d ' ')"
        if [[ "$line_count" -gt 400 ]]; then
            report_failure "$file is a new Rust file with $line_count lines; start below 400 lines and split responsibilities early."
        fi
    done < <(git diff --cached --name-only --diff-filter=A '*.rs')
}

check_production_rust_panics() {
    local file matches
    for file in "${rust_files[@]}"; do
        [[ -f "$file" ]] || continue
        [[ "$file" == tests/* ]] && continue
        [[ "$file" == *"_tests.rs" ]] && continue

        matches="$(
            awk '
                /^#\[cfg\(test\)\]/ { in_tests = 1 }
                !in_tests && /(^|[^[:alnum:]_])(unwrap|expect)[[:space:]]*\(|panic![[:space:]]*\(|todo![[:space:]]*\(|unimplemented![[:space:]]*\(/ {
                    print FILENAME ":" FNR ":" $0
                }
            ' "$file"
        )"
        if [[ -n "$matches" ]]; then
            report_failure "production Rust must not add unchecked unwrap/expect/panic/todo/unimplemented calls:
$matches"
        fi
    done
}

check_generated_outputs_not_staged() {
    local generated
    if [[ "$mode" != "--pre-commit" ]]; then
        return
    fi

    generated="$(printf '%s\n' "${changed_files[@]}" | grep -E '^(dist/|target/|node_modules/)' || true)"
    if [[ -n "$generated" ]]; then
        report_failure "generated/dependency outputs are staged; keep commits focused on source and docs:
$generated"
    fi
}

run_format_and_lint() {
    cargo fmt --all --check
    cargo clippy --all-targets --all-features -- -D warnings
    git diff --check
}

check_file_bloat
check_new_file_size
check_production_rust_panics
check_generated_outputs_not_staged

if [[ "$failures" -ne 0 ]]; then
    exit 1
fi

run_format_and_lint
