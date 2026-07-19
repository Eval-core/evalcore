//! Dataset loading. v0 format: JSONL, one test case per line.

use std::fmt;
use std::path::Path;

use anyhow::Context;
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

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
    /// RAG context: a single `"string"` or an array `["chunk", ...]`. Anything
    /// else (a number, an object, a mixed array) is a dataset error naming the
    /// case's line. An empty array normalizes to `None`.
    #[serde(default, deserialize_with = "deserialize_context")]
    context: Option<Vec<String>>,
}

/// Accepts a single string (→ one-chunk vec) or an array of strings, rejecting
/// any other shape so a malformed `context` fails the case with a clear type
/// error. An empty array normalizes to `None`.
fn deserialize_context<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct ContextVisitor;

    impl<'de> Visitor<'de> for ContextVisitor {
        type Value = Option<Vec<String>>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a context string or an array of context strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(vec![value.to_owned()]))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(vec![value]))
        }

        // A null `context` is treated as absent rather than an error.
        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut chunks = Vec::new();
            // A non-string element (number, object, nested array) surfaces here
            // as an "invalid type" error, rejecting mixed arrays.
            while let Some(chunk) = seq.next_element::<String>()? {
                chunks.push(chunk);
            }
            Ok((!chunks.is_empty()).then_some(chunks))
        }
    }

    deserializer.deserialize_any(ContextVisitor)
}

/// Load a JSONL dataset. Blank lines are skipped; cases without an `id` get
/// `case-<line number>` so results stay addressable. Every case needs an
/// `input` (invoked targets) or a `trace` path (trace targets); trace paths
/// resolve relative to the dataset file.
pub fn load_jsonl(path: &Path) -> anyhow::Result<Vec<TestCase>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read dataset {}", path.display()))?;
    let dataset_dir = path.parent().unwrap_or(Path::new("."));

    // Case ids must be unique: baseline matching and matrix comparison rows
    // are keyed by id, so a duplicate would silently collapse two cases into
    // one row. Fail at load with the offending line instead.
    let mut seen_ids: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
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
            let id = raw.id.unwrap_or_else(|| format!("case-{line_no}"));
            if let Some(first_line) = seen_ids.insert(id.clone(), line_no) {
                anyhow::bail!(
                    "duplicate case id {id:?} at {}:{line_no} (first used at line {first_line})",
                    path.display()
                );
            }
            Ok(TestCase {
                id,
                input: raw.input.unwrap_or_default(),
                expected: raw.expected,
                trace: raw.trace.map(|t| dataset_dir.join(t)),
                context: raw.context,
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
    fn context_accepts_single_string_array_and_normalizes_empty() {
        let (_dir, path) = write_dataset(
            r#"{"id": "one", "input": "q", "context": "just one chunk"}
{"id": "many", "input": "q", "context": ["chunk a", "chunk b"]}
{"id": "empty", "input": "q", "context": []}
{"id": "none", "input": "q"}
"#,
        );
        let cases = load_jsonl(&path).unwrap();
        assert_eq!(
            cases[0].context.as_deref(),
            Some(["just one chunk".to_string()].as_slice()),
            "single string becomes a one-element vec"
        );
        assert_eq!(
            cases[1].context.as_deref(),
            Some(["chunk a".to_string(), "chunk b".to_string()].as_slice()),
            "array is preserved in order"
        );
        assert_eq!(cases[2].context, None, "empty array normalizes to None");
        assert_eq!(cases[3].context, None, "absent context is None");
    }

    #[test]
    fn context_rejects_non_string_shapes_naming_the_case() {
        // A bare number.
        let (_dir, path) = write_dataset(r#"{"id": "bad", "input": "q", "context": 42}"#);
        let err = load_jsonl(&path).unwrap_err();
        assert!(err.to_string().contains(":1"), "names the line; got: {err}");

        // A mixed array (string then number) is rejected element-by-element.
        let (_dir, path) =
            write_dataset("{\"input\": \"ok\"}\n{\"input\": \"q\", \"context\": [\"a\", 7]}\n");
        let err = load_jsonl(&path).unwrap_err();
        assert!(err.to_string().contains(":2"), "names the line; got: {err}");
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

    #[test]
    fn rejects_duplicate_case_ids_naming_both_lines() {
        let (_dir, path) =
            write_dataset("{\"id\": \"x\", \"input\": \"a\"}\n{\"id\": \"x\", \"input\": \"b\"}\n");
        let err = load_jsonl(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("duplicate case id \"x\""), "got: {msg}");
        assert!(msg.contains(":2"), "got: {msg}");
        assert!(msg.contains("first used at line 1"), "got: {msg}");
    }
}
