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
        let trajectory: Trajectory = serde_json::from_value(value)
            .context("trace file has `steps` but is not valid native trajectory format")?;
        return Ok(NormalizedTrace {
            trajectory,
            tokens: None,
            latency_ms: 0,
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
    start_nanos: Option<u128>,
    end_nanos: Option<u128>,
    attributes: serde_json::Map<String, serde_json::Value>,
}

fn normalize_otel(value: &serde_json::Value) -> anyhow::Result<NormalizedTrace> {
    let mut spans = collect_spans(value);
    // Chronological order; spans without timestamps sink to the end in their
    // document order (stable sort), keeping the result deterministic.
    spans.sort_by_key(|s| s.start_nanos.unwrap_or(u128::MAX));

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
    })
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
