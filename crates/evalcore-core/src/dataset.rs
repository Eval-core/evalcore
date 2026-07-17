//! Dataset loading. v0 format: JSONL, one test case per line.

use std::path::Path;

use anyhow::Context;
use serde::Deserialize;

use crate::types::TestCase;

#[derive(Deserialize)]
struct RawCase {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    expected: Option<serde_json::Value>,
    #[serde(default)]
    trace: Option<std::path::PathBuf>,
}

/// Load a JSONL dataset. Blank lines are skipped; cases without an `id` get
/// `case-<line number>` so results stay addressable. Every case needs an
/// `input` (invoked targets) or a `trace` path (trace targets); trace paths
/// resolve relative to the dataset file.
pub fn load_jsonl(path: &Path) -> anyhow::Result<Vec<TestCase>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read dataset {}", path.display()))?;
    let dataset_dir = path.parent().unwrap_or(Path::new("."));

    content
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(idx, line)| {
            let line_no = idx + 1;
            let raw: RawCase = serde_json::from_str(line)
                .with_context(|| format!("invalid case at {}:{line_no}", path.display()))?;
            if raw.input.is_none() && raw.trace.is_none() {
                anyhow::bail!(
                    "case at {}:{line_no} has neither `input` nor `trace`",
                    path.display()
                );
            }
            Ok(TestCase {
                id: raw.id.unwrap_or_else(|| format!("case-{line_no}")),
                input: raw.input.unwrap_or_default(),
                expected: raw.expected,
                trace: raw.trace.map(|t| dataset_dir.join(t)),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_dataset(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cases.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        (dir, path)
    }

    #[test]
    fn loads_cases_and_fills_missing_ids() {
        let (_dir, path) = write_dataset(
            r#"{"id": "named", "input": "hello", "expected": "HELLO"}

{"input": "anonymous"}
"#,
        );
        let cases = load_jsonl(&path).unwrap();
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[0].id, "named");
        assert_eq!(cases[0].expected, Some(serde_json::json!("HELLO")));
        assert_eq!(cases[1].id, "case-3", "id derived from line number");
        assert_eq!(cases[1].expected, None);
    }

    #[test]
    fn trace_cases_resolve_relative_to_dataset_and_need_input_or_trace() {
        let (_dir, path) = write_dataset(
            r#"{"id": "flow", "trace": "traces/run1.json"}
"#,
        );
        let cases = load_jsonl(&path).unwrap();
        assert_eq!(
            cases[0].trace.as_deref(),
            Some(path.parent().unwrap().join("traces/run1.json").as_path())
        );
        assert_eq!(cases[0].input, "");

        let (_dir, path) = write_dataset(r#"{"id": "empty"}"#);
        let err = load_jsonl(&path).unwrap_err();
        assert!(
            err.to_string().contains("neither `input` nor `trace`"),
            "got: {err}"
        );
    }

    #[test]
    fn reports_file_and_line_on_bad_case() {
        let (_dir, path) = write_dataset("{\"input\": \"ok\"}\nnot json\n");
        let err = load_jsonl(&path).unwrap_err();
        assert!(err.to_string().contains(":2"), "got: {err}");
    }

    #[test]
    fn missing_file_names_the_path() {
        let err = load_jsonl(Path::new("/nonexistent/cases.jsonl")).unwrap_err();
        assert!(err.to_string().contains("/nonexistent/cases.jsonl"));
    }
}
