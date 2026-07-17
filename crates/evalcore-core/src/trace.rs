//! Agent-trace ingestion: turn a recorded trace file into a canonical
//! trajectory that scorers can assert on.
//!
//! Two input shapes are accepted, auto-detected:
//!
//! 1. **Native trajectory format** (EvalCore's spec, `docs/trajectory-spec.md`):
//!    `{"steps": [{"tool": "search_kb", "input": {...}, "output": ...}, ...]}`
//! 2. **OTel JSON export** (`{"resourceSpans": [...]}`), reading both OTel
//!    GenAI semantic conventions (`gen_ai.tool.name`, `gen_ai.usage.*`) and
//!    OpenInference conventions (`openinference.span.kind == "TOOL"`,
//!    `tool.name`, `input.value`, `llm.token_count.*`).
//!
//! The design bet (PRD §6.5): apps don't integrate with EvalCore — they emit
//! telemetry they likely already emit, and EvalCore evaluates the export.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

use crate::types::TokenUsage;

/// One tool call in a normalized trajectory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStep {
    pub tool: String,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
}

/// The canonical trajectory: what `trace` targets output (as JSON text) and
/// what the `trajectory` scorer parses back.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trajectory {
    pub steps: Vec<TraceStep>,
}

/// Everything extracted from one trace file.
#[derive(Debug, Clone)]
pub struct NormalizedTrace {
    pub trajectory: Trajectory,
    pub tokens: Option<TokenUsage>,
    /// Wall-clock span of the trace (max end − min start), when timestamps
    /// are present; 0 otherwise.
    pub latency_ms: u64,
    /// The agent's final answer, when the trace carries one: the native
    /// format's top-level `final_output`, or the OTel root span's output
    /// attribute. `None` when the trace records only steps. When present, the
    /// `trace` target grades it (with judge/text scorers) instead of the
    /// trajectory JSON.
    pub final_output: Option<String>,
}

/// Parse canonical trajectory JSON (the `trace` target's output text).
pub fn parse_trajectory(json: &str) -> anyhow::Result<Trajectory> {
    serde_json::from_str(json).context("output is not canonical trajectory JSON")
}

/// Auto-detect and normalize a raw trace file.
pub fn normalize_trace(raw: &str) -> anyhow::Result<NormalizedTrace> {
    let value: serde_json::Value =
        serde_json::from_str(raw).context("trace file is not valid JSON")?;
    if value.get("steps").is_some() {
        // The final answer is an optional top-level string; read it before the
        // trajectory deserialize consumes `value`. Absent/null → None; a
        // present non-string is a data error, not a silent drop.
        let final_output = match value.get("final_output") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(text)) => Some(text.clone()),
            Some(_) => bail!("trace file `final_output` must be a string"),
        };
        let trajectory: Trajectory = serde_json::from_value(value)
            .context("trace file has `steps` but is not valid native trajectory format")?;
        return Ok(NormalizedTrace {
            trajectory,
            tokens: None,
            latency_ms: 0,
            final_output,
        });
    }
    if value.get("resourceSpans").is_some() {
        return normalize_otel(&value);
    }
    bail!(
        "unrecognized trace format: expected native trajectory JSON ({{\"steps\": [...]}}) \
         or an OTel JSON export ({{\"resourceSpans\": [...]}})"
    )
}

/// One flattened OTel span, before tool-call classification.
struct Span {
    name: String,
    /// This span's id; empty when the export omits it.
    span_id: String,
    /// The parent span's id; empty when absent (a root span).
    parent_span_id: String,
    start_nanos: Option<u128>,
    end_nanos: Option<u128>,
    attributes: serde_json::Map<String, serde_json::Value>,
}

fn normalize_otel(value: &serde_json::Value) -> anyhow::Result<NormalizedTrace> {
    let mut spans = collect_spans(value);
    // Chronological order; spans without timestamps sink to the end in their
    // document order (stable sort), keeping the result deterministic.
    spans.sort_by_key(|s| s.start_nanos.unwrap_or(u128::MAX));

    let final_output = root_final_output(&spans);

    let mut steps = Vec::new();
    let mut tokens = TokenUsage::default();
    let mut saw_tokens = false;
    let (mut min_start, mut max_end) = (u128::MAX, 0u128);

    for span in &spans {
        if let (Some(start), Some(end)) = (span.start_nanos, span.end_nanos) {
            min_start = min_start.min(start);
            max_end = max_end.max(end);
        }

        // Token usage accumulates across ALL spans (LLM spans aren't tool
        // spans, but their cost belongs to the run).
        for (input_key, output_key) in [
            ("gen_ai.usage.input_tokens", "gen_ai.usage.output_tokens"),
            ("llm.token_count.prompt", "llm.token_count.completion"),
        ] {
            let input = attr_u64(&span.attributes, input_key);
            let output = attr_u64(&span.attributes, output_key);
            if input.is_some() || output.is_some() {
                saw_tokens = true;
                tokens.input += input.unwrap_or(0);
                tokens.output += output.unwrap_or(0);
            }
        }

        if let Some(tool) = tool_name(span) {
            steps.push(TraceStep {
                tool,
                input: attr_payload(
                    &span.attributes,
                    &["gen_ai.tool.call.arguments", "input.value"],
                )
                .unwrap_or(serde_json::Value::Null),
                output: attr_payload(
                    &span.attributes,
                    &["gen_ai.tool.call.result", "output.value"],
                ),
            });
        }
    }

    let latency_ms = if min_start < max_end {
        ((max_end - min_start) / 1_000_000) as u64
    } else {
        0
    };

    Ok(NormalizedTrace {
        trajectory: Trajectory { steps },
        tokens: saw_tokens.then_some(tokens),
        latency_ms,
        final_output,
    })
}

/// The agent's final answer, extracted from a root span.
///
/// A root candidate is a span whose `parentSpanId` is empty/absent or
/// references a span not present in the export (a partial trace rooted at a
/// dropped parent). Among the candidates that actually carry a final-answer
/// attribute, the one with the LATEST `startTimeUnixNano` wins: on a flat
/// export where every span is a root candidate (e.g. a planner LLM at t=0
/// emitting an interim thought, then a responder at t=5 emitting the answer),
/// the final answer is the last thing said, not the first. For a proper
/// single-root trace this is identical to picking the sole candidate.
///
/// Within a span, extraction precedence is OpenInference `output.value`, then
/// OTel GenAI `gen_ai.completion`. The value stays a raw string — a
/// stringified-JSON answer is NOT unwrapped, since the final answer is text,
/// not a payload to address fields on. No candidate carries one → None.
///
/// Determinism: `spans` arrives already sorted ascending by start time, and
/// `max_by_key` returns the last maximum, so ties resolve to the last
/// candidate in that stable order.
fn root_final_output(spans: &[Span]) -> Option<String> {
    let ids: std::collections::HashSet<&str> = spans
        .iter()
        .map(|s| s.span_id.as_str())
        .filter(|id| !id.is_empty())
        .collect();
    spans
        .iter()
        .filter(|s| s.parent_span_id.is_empty() || !ids.contains(s.parent_span_id.as_str()))
        .filter_map(|s| {
            let answer = ["output.value", "gen_ai.completion"]
                .into_iter()
                .find_map(|key| attr_str(&s.attributes, key))?;
            // Missing timestamps sink to earliest so a timestamped answer wins.
            Some((s.start_nanos.unwrap_or(0), answer))
        })
        .max_by_key(|(start, _)| *start)
        .map(|(_, answer)| answer)
}

/// A span is a tool call if it carries an explicit tool-name attribute
/// (OTel GenAI) or is marked as a TOOL span (OpenInference).
fn tool_name(span: &Span) -> Option<String> {
    if let Some(name) = attr_str(&span.attributes, "gen_ai.tool.name") {
        return Some(name);
    }
    let kind = attr_str(&span.attributes, "openinference.span.kind")?;
    if !kind.eq_ignore_ascii_case("tool") {
        return None;
    }
    Some(attr_str(&span.attributes, "tool.name").unwrap_or_else(|| span.name.clone()))
}

fn collect_spans(value: &serde_json::Value) -> Vec<Span> {
    let mut spans = Vec::new();
    let resource_spans = value["resourceSpans"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for resource in &resource_spans {
        for scope in resource["scopeSpans"].as_array().unwrap_or(&Vec::new()) {
            for span in scope["spans"].as_array().unwrap_or(&Vec::new()) {
                let mut attributes = serde_json::Map::new();
                for attr in span["attributes"].as_array().unwrap_or(&Vec::new()) {
                    if let Some(key) = attr["key"].as_str() {
                        attributes.insert(key.to_string(), unwrap_any_value(&attr["value"]));
                    }
                }
                spans.push(Span {
                    name: span["name"].as_str().unwrap_or_default().to_string(),
                    span_id: span["spanId"].as_str().unwrap_or_default().to_string(),
                    parent_span_id: span["parentSpanId"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    start_nanos: nanos(&span["startTimeUnixNano"]),
                    end_nanos: nanos(&span["endTimeUnixNano"]),
                    attributes,
                });
            }
        }
    }
    spans
}

/// OTLP JSON wraps values as `{"stringValue": ...}`, `{"intValue": "42"}`, …
fn unwrap_any_value(value: &serde_json::Value) -> serde_json::Value {
    for key in ["stringValue", "intValue", "doubleValue", "boolValue"] {
        if let Some(inner) = value.get(key) {
            return inner.clone();
        }
    }
    value.clone()
}

/// OTLP JSON encodes 64-bit timestamps as strings.
fn nanos(value: &serde_json::Value) -> Option<u128> {
    match value {
        serde_json::Value::String(s) => s.parse().ok(),
        serde_json::Value::Number(n) => n.as_u64().map(u128::from),
        _ => None,
    }
}

fn attr_str(attrs: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    attrs.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn attr_u64(attrs: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<u64> {
    let value = attrs.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

/// Tool inputs/outputs are often JSON serialized into a string attribute; if
/// the string parses as JSON, unwrap it so `with:` matchers see fields.
fn attr_payload(
    attrs: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<serde_json::Value> {
    for key in keys {
        if let Some(value) = attrs.get(*key) {
            if let Some(text) = value.as_str() {
                return Some(serde_json::from_str(text).unwrap_or(value.clone()));
            }
            return Some(value.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_format_roundtrips() {
        let raw = r#"{"steps": [
            {"tool": "search_kb", "input": {"query": "refund policy"}},
            {"tool": "reply", "input": {"text": "30 days"}, "output": "sent"}
        ]}"#;
        let normalized = normalize_trace(raw).unwrap();
        assert_eq!(normalized.trajectory.steps.len(), 2);
        assert_eq!(normalized.trajectory.steps[0].tool, "search_kb");
        assert_eq!(
            normalized.trajectory.steps[1].output,
            Some(serde_json::json!("sent"))
        );

        let json = serde_json::to_string(&normalized.trajectory).unwrap();
        assert_eq!(parse_trajectory(&json).unwrap().steps.len(), 2);
        assert_eq!(
            normalized.final_output, None,
            "native trace without final_output leaves it None"
        );
    }

    #[test]
    fn native_final_output_is_extracted() {
        let raw = r#"{"final_output": "Refunds take 30 days.",
            "steps": [{"tool": "search_kb", "input": {}}]}"#;
        let normalized = normalize_trace(raw).unwrap();
        assert_eq!(
            normalized.final_output.as_deref(),
            Some("Refunds take 30 days.")
        );
        assert_eq!(normalized.trajectory.steps.len(), 1, "steps still parse");
    }

    /// One test span: `(name, spanId, parentId, [(attr, value)])`.
    type SpanSpec<'a> = (&'a str, &'a str, &'a str, Vec<(&'a str, &'a str)>);

    /// Build a minimal OTel export from span specs. Timestamps are assigned in
    /// argument order.
    fn otel_export(spans: &[SpanSpec]) -> String {
        let spans: Vec<serde_json::Value> = spans
            .iter()
            .enumerate()
            .map(|(i, (name, id, parent, attrs))| {
                let start = (i as u64 + 1) * 1_000_000_000;
                let attributes: Vec<serde_json::Value> = attrs
                    .iter()
                    .map(|(k, v)| serde_json::json!({"key": k, "value": {"stringValue": v}}))
                    .collect();
                serde_json::json!({
                    "name": name,
                    "spanId": id,
                    "parentSpanId": parent,
                    "startTimeUnixNano": start.to_string(),
                    "endTimeUnixNano": (start + 500_000_000).to_string(),
                    "attributes": attributes,
                })
            })
            .collect();
        serde_json::json!({ "resourceSpans": [{"scopeSpans": [{"spans": spans}]}] }).to_string()
    }

    #[test]
    fn otel_root_output_openinference_convention() {
        // Root span (empty parent) carries an OpenInference output.value.
        let raw = otel_export(&[
            (
                "agent",
                "root",
                "",
                vec![("output.value", "The answer is 30 days.")],
            ),
            (
                "execute_tool search_kb",
                "s1",
                "root",
                vec![("gen_ai.tool.name", "search_kb")],
            ),
        ]);
        let normalized = normalize_trace(&raw).unwrap();
        assert_eq!(
            normalized.final_output.as_deref(),
            Some("The answer is 30 days.")
        );
    }

    #[test]
    fn otel_root_output_genai_convention() {
        let raw = otel_export(&[(
            "agent",
            "root",
            "",
            vec![("gen_ai.completion", "GenAI answer")],
        )]);
        let normalized = normalize_trace(&raw).unwrap();
        assert_eq!(normalized.final_output.as_deref(), Some("GenAI answer"));
    }

    #[test]
    fn otel_root_output_precedence_prefers_openinference() {
        // Both conventions present on the root: output.value wins.
        let raw = otel_export(&[(
            "agent",
            "root",
            "",
            vec![
                ("gen_ai.completion", "genai wins?"),
                ("output.value", "openinference wins"),
            ],
        )]);
        let normalized = normalize_trace(&raw).unwrap();
        assert_eq!(
            normalized.final_output.as_deref(),
            Some("openinference wins")
        );
    }

    #[test]
    fn otel_root_output_absent_is_none() {
        let raw = otel_export(&[("agent", "root", "", vec![("gen_ai.tool.name", "search_kb")])]);
        assert_eq!(normalize_trace(&raw).unwrap().final_output, None);
    }

    #[test]
    fn otel_root_latest_outputting_candidate_wins_on_flat_trace() {
        // Flat trace: every span is a root candidate. A planner LLM speaks
        // first (an interim thought), the responder speaks last (the answer).
        // The LATEST-starting candidate carrying an output attribute is the
        // final answer — grading the planner's thought would be wrong.
        let raw = otel_export(&[
            (
                "planner",
                "",
                "",
                vec![("output.value", "I should search the KB")],
            ),
            (
                "responder",
                "",
                "",
                vec![("output.value", "Refunds take 30 days")],
            ),
        ]);
        // otel_export assigns start times in argument order, so "responder"
        // starts later and must win.
        let normalized = normalize_trace(&raw).unwrap();
        assert_eq!(
            normalized.final_output.as_deref(),
            Some("Refunds take 30 days"),
            "the last outputting candidate is the final answer, not the first"
        );
    }

    #[test]
    fn native_non_string_final_output_is_an_error() {
        let raw = r#"{"final_output": {"answer": 42}, "steps": []}"#;
        let err = normalize_trace(raw).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("final_output"), "got: {msg}");
        assert!(msg.contains("must be a string"), "got: {msg}");
    }

    #[test]
    fn example_native_fixture_final_output_is_pinned() {
        // Guards the shipped fixture and the extraction against silent drift:
        // the E2E `contains: "30 days"` check would still pass on a changed
        // answer that coincidentally contains "30 days".
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/agent-trace/traces/refund-native.json"
        );
        let raw = std::fs::read_to_string(path).unwrap();
        let normalized = normalize_trace(&raw).unwrap();
        assert_eq!(
            normalized.final_output.as_deref(),
            Some("Yes — refunds are honored within 30 days of purchase.")
        );
    }

    #[test]
    fn example_otel_fixture_final_output_is_pinned() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/agent-trace/traces/refund-otel.json"
        );
        let raw = std::fs::read_to_string(path).unwrap();
        let normalized = normalize_trace(&raw).unwrap();
        assert_eq!(
            normalized.final_output.as_deref(),
            Some("Yes — refunds are honored within 30 days of purchase.")
        );
    }

    #[test]
    fn otel_root_is_span_with_dangling_parent() {
        // No empty-parent span: the export is a sub-tree whose parent ("gone")
        // is not present. That span is the root; its output is extracted.
        let raw = otel_export(&[
            ("child", "c1", "root-span", vec![("gen_ai.tool.name", "t")]),
            (
                "subtree-root",
                "root-span",
                "gone",
                vec![("output.value", "dangling-parent answer")],
            ),
        ]);
        let normalized = normalize_trace(&raw).unwrap();
        assert_eq!(
            normalized.final_output.as_deref(),
            Some("dangling-parent answer"),
            "a parentSpanId referencing a missing span marks the root"
        );
    }

    fn otel_fixture() -> String {
        // Three spans, deliberately out of chronological order in the
        // document: an LLM span (tokens, no tool), then reply, then search.
        serde_json::json!({
            "resourceSpans": [{"scopeSpans": [{"spans": [
                {
                    "name": "chat gpt-4.1",
                    "startTimeUnixNano": "1000000000",
                    "endTimeUnixNano": "2000000000",
                    "attributes": [
                        {"key": "gen_ai.usage.input_tokens", "value": {"intValue": "100"}},
                        {"key": "gen_ai.usage.output_tokens", "value": {"intValue": "25"}}
                    ]
                },
                {
                    "name": "reply",
                    "startTimeUnixNano": "5000000000",
                    "endTimeUnixNano": "6000000000",
                    "attributes": [
                        {"key": "openinference.span.kind", "value": {"stringValue": "TOOL"}},
                        {"key": "tool.name", "value": {"stringValue": "reply"}},
                        {"key": "input.value", "value": {"stringValue": "{\"text\": \"30 days\"}"}},
                        {"key": "llm.token_count.prompt", "value": {"intValue": "10"}},
                        {"key": "llm.token_count.completion", "value": {"intValue": "5"}}
                    ]
                },
                {
                    "name": "execute_tool search_kb",
                    "startTimeUnixNano": "3000000000",
                    "endTimeUnixNano": "4000000000",
                    "attributes": [
                        {"key": "gen_ai.tool.name", "value": {"stringValue": "search_kb"}},
                        {"key": "gen_ai.tool.call.arguments", "value": {"stringValue": "{\"query\": \"refund policy\"}"}}
                    ]
                }
            ]}]}]
        })
        .to_string()
    }

    #[test]
    fn otel_spans_normalize_ordered_with_tokens_and_latency() {
        let normalized = normalize_trace(&otel_fixture()).unwrap();

        let tools: Vec<_> = normalized
            .trajectory
            .steps
            .iter()
            .map(|s| s.tool.as_str())
            .collect();
        assert_eq!(
            tools,
            ["search_kb", "reply"],
            "steps ordered by start time, LLM span excluded"
        );
        assert_eq!(
            normalized.trajectory.steps[0].input["query"],
            serde_json::json!("refund policy"),
            "stringified JSON arguments are unwrapped"
        );

        let tokens = normalized.tokens.unwrap();
        assert_eq!(
            (tokens.input, tokens.output),
            (110, 30),
            "usage summed across GenAI and OpenInference conventions"
        );
        assert_eq!(normalized.latency_ms, 5000, "max end - min start in ms");
    }

    #[test]
    fn unknown_shapes_name_both_supported_formats() {
        let err = normalize_trace(r#"{"whatever": []}"#).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("steps"), "got: {msg}");
        assert!(msg.contains("resourceSpans"), "got: {msg}");

        assert!(normalize_trace("not json").is_err());
    }
}
