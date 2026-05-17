use std::collections::HashMap;

pub(crate) fn row_value(
    row: &[String],
    keys: &HashMap<String, usize>,
    candidates: &[&str],
) -> Option<String> {
    for candidate in candidates {
        let Some(index) = keys.get(&normalize_field_name(candidate)) else {
            continue;
        };
        let Some(value) = row.get(*index) else {
            continue;
        };
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    None
}

pub(crate) fn parse_csv_rows(input: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if in_quotes && matches!(chars.peek(), Some('"')) {
                    field.push('"');
                    let _ = chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => {
                row.push(field.trim().to_owned());
                field.clear();
            }
            '\n' if !in_quotes => {
                row.push(field.trim().to_owned());
                field.clear();
                if !row.iter().all(|cell| cell.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            '\r' => {}
            _ => field.push(ch),
        }
    }

    if !field.is_empty() || !row.is_empty() {
        row.push(field.trim().to_owned());
        if !row.iter().all(|cell| cell.is_empty()) {
            rows.push(row);
        }
    }

    rows
}

pub(crate) fn normalize_field_name(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
