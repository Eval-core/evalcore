//! `evalcore serve`: a local, read-only web viewer over the run-history stored
//! in an `.evalcore/cache.db`. It only ever reads the store — there are no
//! mutation endpoints, and the binder is hard-coded to `127.0.0.1`, which is
//! the entire security model (localhost-only, so no auth).
//!
//! Routes are all `GET` (a non-GET method gets a 405 from axum's method
//! router). Run detail and diff pages reuse the `evalcore-report` renderers
//! verbatim, so a run page equals the `--html` report and a diff equals the
//! matrix comparison; only the listing chrome is rendered in [`render`].

mod render;

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use anyhow::Context;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use evalcore_core::{compare_arms, MatrixArm, MatrixSummary};
use evalcore_store::{RunMeta, Store};

/// Build the viewer's router over a shared, read-only [`Store`]. Pure wiring —
/// no socket is bound here, so tests drive it with `tower::ServiceExt::oneshot`.
/// Path parameters use axum 0.8 brace syntax (`{id}`).
pub fn router(store: Arc<Store>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/run/{id}", get(run_detail))
        .route("/diff", get(diff))
        .fallback(not_found)
        .with_state(store)
}

/// Bind `127.0.0.1:<port>` (localhost only — never a wildcard address) and
/// serve the viewer until the process is interrupted. The address is not
/// configurable by design.
pub async fn run(store: Arc<Store>, port: u16) -> anyhow::Result<()> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    axum::serve(listener, router(store))
        .await
        .context("serve loop failed")?;
    Ok(())
}

/// `GET /` — the run-history listing. A store read error (not a single corrupt
/// row, which [`render::index_page`] handles inline) is the only 500 here.
async fn index(State(store): State<Arc<Store>>) -> Response {
    match store.list_runs() {
        Ok(runs) => Html(render::index_page(&runs)).into_response(),
        Err(err) => internal_error(&format!("failed to read run history: {err}")),
    }
}

/// `GET /run/{id}` — full run detail, rendered by `evalcore_report::html` so the
/// page is byte-equal to that run's `--html` report. A non-integer id → styled
/// 400 (parsed here rather than via `Path<i64>`, whose rejection is axum's
/// bare plaintext); unknown id → 404; a corrupt stored summary → 500.
async fn run_detail(State(store): State<Arc<Store>>, Path(id): Path<String>) -> Response {
    let Ok(id) = id.parse::<i64>() else {
        return (
            StatusCode::BAD_REQUEST,
            Html(render::bad_request_page("run id must be an integer")),
        )
            .into_response();
    };
    match store.load_run(id) {
        Ok(Some(meta)) => match meta.summary {
            Ok(summary) => Html(evalcore_report::html(&summary, None)).into_response(),
            Err(message) => internal_error(&message),
        },
        Ok(None) => not_found_id(id),
        Err(err) => internal_error(&format!("failed to load run {id}: {err}")),
    }
}

/// `GET /diff?a=<id>&b=<id>` — side-by-side comparison of any two stored runs,
/// rendered with the matrix comparison renderer. Missing/invalid `a`/`b` →
/// axum's own 400 (via [`Query`]); an unknown id → 404; a corrupt summary → 500.
async fn diff(
    State(store): State<Arc<Store>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (a, b) = match (parse_id(&params, "a"), parse_id(&params, "b")) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Html(render::bad_request_page(
                    "diff needs integer query parameters a and b, e.g. /diff?a=1&b=2",
                )),
            )
                .into_response()
        }
    };

    let arm_a = match load_arm(&store, a) {
        Ok(arm) => arm,
        Err(response) => return *response,
    };
    let arm_b = match load_arm(&store, b) {
        Ok(arm) => arm,
        Err(response) => return *response,
    };

    let matrix = MatrixSummary {
        arms: vec![arm_a, arm_b],
    };
    let comparison = compare_arms(&matrix);
    Html(evalcore_report::html_matrix(&matrix, &comparison)).into_response()
}

/// Load one run as a matrix arm named `run #<id> (<target>)`. On a miss/corrupt
/// row/DB error, returns the ready-to-send error [`Response`] in `Err` (boxed —
/// an axum `Response` is large) so the caller can short-circuit.
fn load_arm(store: &Store, id: i64) -> Result<MatrixArm, Box<Response>> {
    match store.load_run(id) {
        Ok(Some(meta)) => arm_from_meta(meta),
        Ok(None) => Err(Box::new(not_found_id(id))),
        Err(err) => Err(Box::new(internal_error(&format!(
            "failed to load run {id}: {err}"
        )))),
    }
}

/// Turn a loaded [`RunMeta`] into a named arm, or a boxed 500 response if its
/// summary is corrupt. The arm name carries the id and target; `html_matrix`
/// escapes it.
fn arm_from_meta(meta: RunMeta) -> Result<MatrixArm, Box<Response>> {
    let RunMeta {
        id,
        target,
        summary,
        ..
    } = meta;
    match summary {
        Ok(summary) => Ok(MatrixArm {
            target: format!("run #{id} ({target})"),
            summary,
        }),
        Err(message) => Err(Box::new(internal_error(&message))),
    }
}

/// Parse a query parameter as an `i64`; `None` when absent or not an integer.
fn parse_id(params: &HashMap<String, String>, key: &str) -> Option<i64> {
    params.get(key)?.parse().ok()
}

/// The catch-all 404 for any unmatched route.
async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Html(render::not_found_page("no such page")),
    )
        .into_response()
}

fn not_found_id(id: i64) -> Response {
    (
        StatusCode::NOT_FOUND,
        Html(render::not_found_page(&format!("no run #{id}"))),
    )
        .into_response()
}

fn internal_error(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Html(render::server_error_page(message)),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use evalcore_core::{CaseResult, RunSummary, Score, TargetOutput};
    use tower::ServiceExt;

    /// A one-case summary: `case_id` passes iff `passed`, carrying a `contains`
    /// score so the diff comparison has a value to rank.
    fn summary(case_id: &str, passed: bool) -> RunSummary {
        RunSummary {
            results: vec![CaseResult {
                case_id: case_id.into(),
                output: Some(TargetOutput {
                    text: "ok".into(),
                    latency_ms: 5,
                    tokens: None,
                    trajectory: None,
                }),
                error: None,
                scores: vec![Score {
                    scorer: "contains".into(),
                    value: if passed { 1.0 } else { 0.0 },
                    passed,
                    reason: None,
                }],
                cost_usd: None,
                context: None,
                trials: None,
            }],
            gates: Vec::new(),
            classification: None,
        }
    }

    /// A store seeded with two runs; the first target name is hostile to prove
    /// escaping. Returns the store and the temp dir (kept alive by the caller).
    fn seeded() -> (Arc<Store>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join(".evalcore/cache.db")).unwrap();
        store
            .record_run(
                "evals.yaml",
                "<script>alert(1)</script>",
                &summary("case-a", true),
            )
            .unwrap();
        store
            .record_run("evals.yaml", "safe", &summary("case-b", false))
            .unwrap();
        (Arc::new(store), dir)
    }

    async fn get(store: Arc<Store>, uri: &str) -> (StatusCode, String) {
        let response = router(store)
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn index_lists_runs_and_escapes_hostile_target() {
        let (store, _dir) = seeded();
        let (status, body) = get(store, "/").await;
        assert_eq!(status, StatusCode::OK);
        // Both runs listed, newest first (safe = id 2 before the hostile id 1).
        assert!(body.contains("/run/1"), "links run 1; got: {body}");
        assert!(body.contains("/run/2"), "links run 2");
        assert!(body.find("/run/2").unwrap() < body.find("/run/1").unwrap());
        // The hostile target name renders inert.
        assert!(
            body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "hostile target escaped; got: {body}"
        );
        assert!(
            !body.contains("<script>alert(1)"),
            "no live script survives"
        );
    }

    #[tokio::test]
    async fn run_detail_is_the_report_html() {
        let (store, _dir) = seeded();
        let (status, body) = get(store, "/run/1").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("EvalCore report"), "report chrome present");
        assert!(
            body.contains("case-a"),
            "known case id present; got: {body}"
        );
    }

    #[tokio::test]
    async fn diff_reuses_the_matrix_comparison() {
        let (store, _dir) = seeded();
        let (status, body) = get(store, "/diff?a=1&b=2").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Comparison"), "comparison table present");
        // Arm names carry the run ids; the hostile arm name is escaped.
        assert!(body.contains("run #1"), "arm a named; got: {body}");
        assert!(body.contains("run #2"), "arm b named");
        assert!(!body.contains("<script>alert(1)"), "hostile arm escaped");
    }

    #[tokio::test]
    async fn unknown_run_id_is_404() {
        let (store, _dir) = seeded();
        let (status, body) = get(store, "/run/999").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.contains("no run #999"), "got: {body}");
    }

    #[tokio::test]
    async fn diff_with_bad_ids_is_400() {
        let (store, _dir) = seeded();
        let (status, _) = get(store, "/diff?a=notint&b=2").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unknown_route_is_404() {
        let (store, _dir) = seeded();
        let (status, _) = get(store, "/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn non_get_methods_are_405() {
        let (store, _dir) = seeded();
        let response = router(store)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn corrupt_summary_row_is_an_error_entry_not_a_500() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".evalcore/cache.db");
        let store = Store::open(&path).unwrap();
        store
            .record_run("evals.yaml", "ok", &summary("good", true))
            .unwrap();
        // Corrupt a row directly on disk.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO run_history (config, target, summary) VALUES ('evals.yaml','bad','not json')",
            [],
        )
        .unwrap();
        drop(conn);

        let (status, body) = get(Arc::new(store), "/").await;
        assert_eq!(status, StatusCode::OK, "listing still renders");
        assert!(body.contains("/run/1"), "the good row still lists");
        assert!(body.contains("corrupt run-history summary"), "got: {body}");
    }
}
