//! Local SQLite storage. Today: the record/replay cache — every cacheable
//! target call is stored keyed by a content hash of the canonical request, so
//! reruns are free, offline, and deterministic. (Run history for baseline
//! diffing lands here next — PRD §6.6.)
//!
//! The cache file is a project artifact, like a VCR cassette directory:
//! committing `.evalcore/cache.db` lets CI run `--cache replay` with zero
//! LLM spend.

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use evalcore_core::{RunSummary, Target, TargetOutput, TestCase};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

/// How the cache participates in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    /// Replay hits, call live and record on miss. The default.
    Auto,
    /// Cache only: a miss fails the case. Use in CI for deterministic,
    /// zero-cost reruns.
    Replay,
    /// Always call live and overwrite the recording.
    Live,
}

/// Content-address a canonical request. Callers build the canonical string
/// from `serde_json::Value::to_string()`, which sorts object keys (do not
/// enable serde_json's `preserve_order` feature — it would break key
/// stability).
pub fn cache_key(canonical_request: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_request.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One run-history row: the metadata a viewer lists, plus the run's summary
/// parsed lazily. A corrupt summary is carried as `Err(message)` rather than
/// aborting the whole listing — so `evalcore serve` renders that one row as an
/// error entry and every other row still shows. `summary` keeps the run's
/// gates and classification (unlike a baseline, which clears them): they are
/// exactly what the viewer displays.
#[derive(Debug)]
pub struct RunMeta {
    /// Autoincrement row id — stable, newest largest.
    pub id: i64,
    /// SQLite `datetime('now')` at record time (`YYYY-MM-DD HH:MM:SS`, UTC).
    pub created_at: String,
    /// The config file path exactly as the user gave it to `evalcore run`.
    pub config: String,
    /// The target this row records (a matrix arm's name for matrix runs).
    pub target: String,
    /// The stored `RunSummary`, or a message when the row's JSON is corrupt.
    pub summary: Result<RunSummary, String>,
}

/// A SQLite-backed store. Cheap to open; safe to share across concurrent
/// cases (a single connection behind a mutex — cache lookups are microseconds
/// next to LLM calls).
pub struct Store {
    conn: Mutex<rusqlite::Connection>,
}

impl Store {
    /// Open (creating file and parent directories if needed).
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let conn = rusqlite::Connection::open(path)
            .with_context(|| format!("failed to open store at {}", path.display()))?;
        // A second connection to the same file (rare, but tests do it) should
        // wait rather than fail with SQLITE_BUSY.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS llm_cache (
                key        TEXT PRIMARY KEY,
                request    TEXT NOT NULL,
                response   TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS runs (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                label      TEXT NOT NULL,
                summary    TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS run_history (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                config     TEXT NOT NULL,
                target     TEXT NOT NULL,
                summary    TEXT NOT NULL
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn get(&self, key: &str) -> anyhow::Result<Option<TargetOutput>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let row: Option<String> = conn
            .query_row(
                "SELECT response FROM llm_cache WHERE key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        row.map(|json| {
            serde_json::from_str(&json).context("corrupt cache entry — delete the cache file")
        })
        .transpose()
    }

    pub fn put(
        &self,
        key: &str,
        canonical_request: &str,
        output: &TargetOutput,
    ) -> anyhow::Result<()> {
        let response = serde_json::to_string(output)?;
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO llm_cache (key, request, response) VALUES (?1, ?2, ?3)",
            rusqlite::params![key, canonical_request, response],
        )?;
        Ok(())
    }

    /// Persist a run's full results under a baseline label. Labels are not
    /// unique — each save appends, and `load_baseline` returns the newest.
    pub fn save_baseline(&self, label: &str, summary: &RunSummary) -> anyhow::Result<()> {
        let json = serde_json::to_string(summary)?;
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO runs (label, summary) VALUES (?1, ?2)",
            rusqlite::params![label, json],
        )?;
        Ok(())
    }

    /// Load the most recently saved baseline with this label.
    pub fn load_baseline(&self, label: &str) -> anyhow::Result<Option<RunSummary>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let row: Option<String> = conn
            .query_row(
                "SELECT summary FROM runs WHERE label = ?1 ORDER BY id DESC LIMIT 1",
                [label],
                |r| r.get(0),
            )
            .optional()?;
        row.map(|json| {
            serde_json::from_str(&json)
                .context("corrupt baseline entry — delete the .evalcore store file")
        })
        .transpose()
    }

    /// Append one run-history row: the config path as given, the target name
    /// (one call per matrix arm), and the full run summary (gates and
    /// classification included). Rows are append-only and never updated; the
    /// viewer reads them. `created_at` is filled by SQLite — row metadata, never
    /// a cache key or a `run` reporter input.
    pub fn record_run(
        &self,
        config: &str,
        target: &str,
        summary: &RunSummary,
    ) -> anyhow::Result<()> {
        let json = serde_json::to_string(summary)?;
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO run_history (config, target, summary) VALUES (?1, ?2, ?3)",
            rusqlite::params![config, target, json],
        )?;
        Ok(())
    }

    /// Every run-history row, newest first. Each row's summary is parsed here (a
    /// corrupt one becomes `Err` on that [`RunMeta`], not a failure of the whole
    /// call) — cheap at local scale, and it keeps the viewer's listing a pure
    /// read.
    pub fn list_runs(&self) -> anyhow::Result<Vec<RunMeta>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, created_at, config, target, summary FROM run_history ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let mut metas = Vec::new();
        for row in rows {
            let (id, created_at, config, target, json) = row?;
            metas.push(RunMeta {
                id,
                created_at,
                config,
                target,
                summary: parse_summary(&json),
            });
        }
        Ok(metas)
    }

    /// Load one run-history row by id, or `None` when no such row exists. A
    /// corrupt summary is carried on the returned [`RunMeta`] as `Err`, never a
    /// panic — the caller decides how to surface it.
    pub fn load_run(&self, id: i64) -> anyhow::Result<Option<RunMeta>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let row: Option<(String, String, String, String)> = conn
            .query_row(
                "SELECT created_at, config, target, summary FROM run_history WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        Ok(row.map(|(created_at, config, target, json)| RunMeta {
            id,
            created_at,
            config,
            target,
            summary: parse_summary(&json),
        }))
    }
}

/// Parse a stored summary, mapping a corruption to a short message (kept on the
/// [`RunMeta`]) so one bad row never panics or aborts a listing.
fn parse_summary(json: &str) -> Result<RunSummary, String> {
    serde_json::from_str(json).map_err(|err| format!("corrupt run-history summary: {err}"))
}

/// Wraps any target with record/replay behavior. Targets whose
/// `cache_identity()` is `None` (e.g. shell targets) pass straight through in
/// every mode. The store is shared (`Arc`) so the main target and any LLM
/// judges record into the same cache file.
pub struct CachedTarget {
    inner: Box<dyn Target>,
    store: Arc<Store>,
    mode: CacheMode,
}

impl CachedTarget {
    pub fn new(inner: Box<dyn Target>, store: Arc<Store>, mode: CacheMode) -> Self {
        Self { inner, store, mode }
    }
}

#[async_trait]
impl Target for CachedTarget {
    async fn invoke(&self, case: &TestCase) -> anyhow::Result<TargetOutput> {
        let Some(identity) = self.inner.cache_identity() else {
            return self.inner.invoke(case).await;
        };
        // serde_json::Value objects serialize with sorted keys → canonical.
        // Trial 0 (the only trial for single-trial and non-trial runs) uses the
        // pre-trials key shape UNCHANGED, so existing cassettes replay as trial
        // 0. Trials i > 0 add a `"trial": i` salt (BTreeMap key order:
        // identity, input, trial) so each trial re-keys and measures fresh.
        let trial = evalcore_core::engine::current_trial();
        let canonical = if trial == 0 {
            serde_json::json!({
                "identity": identity,
                "input": case.input,
            })
            .to_string()
        } else {
            serde_json::json!({
                "identity": identity,
                "input": case.input,
                "trial": trial,
            })
            .to_string()
        };
        let key = cache_key(&canonical);

        match self.mode {
            CacheMode::Auto => {
                if let Some(hit) = self.store.get(&key)? {
                    return Ok(hit);
                }
                let output = self.inner.invoke(case).await?;
                self.store.put(&key, &canonical, &output)?;
                Ok(output)
            }
            CacheMode::Replay => self.store.get(&key)?.ok_or_else(|| {
                anyhow!(
                    "cache miss for case {:?} in replay mode — record it first with --cache auto (or live)",
                    case.id
                )
            }),
            CacheMode::Live => {
                let output = self.inner.invoke(case).await?;
                self.store.put(&key, &canonical, &output)?;
                Ok(output)
            }
        }
    }

    fn cache_identity(&self) -> Option<serde_json::Value> {
        self.inner.cache_identity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Fake target that counts invocations and answers `call-<n>`.
    struct Counting {
        calls: Arc<AtomicUsize>,
        identity: Option<serde_json::Value>,
    }

    #[async_trait]
    impl Target for Counting {
        async fn invoke(&self, _case: &TestCase) -> anyhow::Result<TargetOutput> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(TargetOutput {
                text: format!("call-{n}"),
                latency_ms: 5,
                tokens: None,
                trajectory: None,
            })
        }

        fn cache_identity(&self) -> Option<serde_json::Value> {
            self.identity.clone()
        }
    }

    fn cached(
        identity: Option<serde_json::Value>,
        store: Store,
        mode: CacheMode,
    ) -> (CachedTarget, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let target = CachedTarget::new(
            Box::new(Counting {
                calls: calls.clone(),
                identity,
            }),
            Arc::new(store),
            mode,
        );
        (target, calls)
    }

    fn case(id: &str, input: &str) -> TestCase {
        TestCase {
            id: id.into(),
            input: input.into(),
            expected: None,
            trace: None,
            context: None,
        }
    }

    fn temp_store(dir: &tempfile::TempDir) -> Store {
        Store::open(&dir.path().join("nested/dir/cache.db")).unwrap()
    }

    fn model_identity() -> Option<serde_json::Value> {
        Some(serde_json::json!({"type": "test", "model": "m1"}))
    }

    #[test]
    fn store_roundtrips_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let store = temp_store(&dir);
        let output = TargetOutput {
            text: "hello".into(),
            latency_ms: 42,
            tokens: None,
            trajectory: None,
        };

        assert!(store.get("k1").unwrap().is_none());
        store.put("k1", "{}", &output).unwrap();
        let hit = store.get("k1").unwrap().unwrap();
        assert_eq!(hit.text, "hello");
        assert_eq!(hit.latency_ms, 42, "replay returns recorded latency");
    }

    #[test]
    fn baselines_roundtrip_and_newest_wins() {
        let dir = tempfile::tempdir().unwrap();
        let store = temp_store(&dir);

        assert!(store.load_baseline("main").unwrap().is_none());

        let older = RunSummary {
            results: vec![evalcore_core::CaseResult {
                case_id: "old".into(),
                output: None,
                error: None,
                scores: vec![],
                cost_usd: None,
                context: None,
                trials: None,
            }],
            gates: Vec::new(),
            classification: None,
        };
        let newer = RunSummary {
            results: vec![evalcore_core::CaseResult {
                case_id: "new".into(),
                output: None,
                error: None,
                scores: vec![],
                cost_usd: None,
                context: None,
                trials: None,
            }],
            gates: Vec::new(),
            classification: None,
        };
        store.save_baseline("main", &older).unwrap();
        store.save_baseline("main", &newer).unwrap();
        store.save_baseline("other", &older).unwrap();

        let loaded = store.load_baseline("main").unwrap().unwrap();
        assert_eq!(loaded.results[0].case_id, "new", "newest save wins");
        let other = store.load_baseline("other").unwrap().unwrap();
        assert_eq!(other.results[0].case_id, "old", "labels are independent");
    }

    /// A one-case summary with the given id and a gate, so tests can assert
    /// that history rows keep run-scoped data (gates/classification) a baseline
    /// would have cleared.
    fn history_summary(case_id: &str, gate_passed: bool) -> RunSummary {
        RunSummary {
            results: vec![evalcore_core::CaseResult {
                case_id: case_id.into(),
                output: None,
                error: None,
                scores: vec![],
                cost_usd: None,
                context: None,
                trials: None,
            }],
            gates: vec![evalcore_core::GateResult {
                gate: "pass_rate >= 0.5".into(),
                actual: 1.0,
                passed: gate_passed,
                reason: None,
            }],
            classification: None,
        }
    }

    #[test]
    fn run_history_records_lists_newest_first_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let store = temp_store(&dir);

        assert!(store.list_runs().unwrap().is_empty());
        assert!(store.load_run(1).unwrap().is_none(), "missing id -> None");

        store
            .record_run("evals.yaml", "gpt", &history_summary("older", true))
            .unwrap();
        store
            .record_run("evals.yaml", "claude", &history_summary("newer", false))
            .unwrap();

        let runs = store.list_runs().unwrap();
        assert_eq!(runs.len(), 2);
        // Newest first: the claude row (id 2) precedes the gpt row (id 1).
        assert_eq!(runs[0].target, "claude");
        assert_eq!(runs[1].target, "gpt");
        assert!(runs[0].id > runs[1].id, "ids descend");
        assert_eq!(runs[0].config, "evals.yaml");

        // History keeps gates (a baseline would have cleared them).
        let newer = runs[0].summary.as_ref().unwrap();
        assert_eq!(newer.gates.len(), 1);
        assert!(!newer.gates[0].passed);
        assert_eq!(newer.results[0].case_id, "newer");

        let loaded = store.load_run(runs[1].id).unwrap().unwrap();
        assert_eq!(loaded.target, "gpt");
        assert_eq!(loaded.summary.unwrap().results[0].case_id, "older");
    }

    #[test]
    fn corrupt_run_history_row_surfaces_as_error_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.db");
        let store = Store::open(&path).unwrap();
        store
            .record_run("evals.yaml", "gpt", &history_summary("ok", true))
            .unwrap();

        // Corrupt the second row's summary directly on disk.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO run_history (config, target, summary) VALUES ('evals.yaml', 'bad', 'not json')",
                [],
            )
            .unwrap();
        }

        let runs = store.list_runs().unwrap();
        assert_eq!(runs.len(), 2, "the whole listing still returns");
        // Newest first: the corrupt row is an Err entry, the good row is Ok.
        let corrupt = runs.iter().find(|r| r.target == "bad").unwrap();
        assert!(
            corrupt.summary.is_err(),
            "corrupt summary is an error entry, not a panic"
        );
        let good = runs.iter().find(|r| r.target == "gpt").unwrap();
        assert!(good.summary.is_ok());

        // load_run of the corrupt id likewise carries the Err, never panics.
        let loaded = store.load_run(corrupt.id).unwrap().unwrap();
        assert!(loaded.summary.is_err());
    }

    #[test]
    fn run_history_table_is_additive_over_a_pre_existing_db() {
        // A cache.db written by an older build (only llm_cache) must reopen and
        // gain run_history without touching the old cache rows.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE llm_cache (
                    key TEXT PRIMARY KEY, request TEXT NOT NULL,
                    response TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                INSERT INTO llm_cache (key, request, response) VALUES ('k', '{}', '{\"text\":\"hi\",\"latency_ms\":1}');",
            )
            .unwrap();
        }

        // Reopen with the current schema: no error, old cache row intact, and
        // run_history now usable.
        let store = Store::open(&path).unwrap();
        assert_eq!(store.get("k").unwrap().unwrap().text, "hi");
        store
            .record_run("evals.yaml", "gpt", &history_summary("c", true))
            .unwrap();
        assert_eq!(store.list_runs().unwrap().len(), 1);
    }

    #[test]
    fn cache_key_is_stable_and_input_sensitive() {
        assert_eq!(cache_key("abc"), cache_key("abc"));
        assert_ne!(cache_key("abc"), cache_key("abd"));
        // Known-answer so an accidental hasher swap breaks loudly.
        assert_eq!(
            cache_key(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[tokio::test]
    async fn auto_mode_records_then_replays() {
        let dir = tempfile::tempdir().unwrap();
        let (target, calls) = cached(model_identity(), temp_store(&dir), CacheMode::Auto);

        let first = target.invoke(&case("a", "hi")).await.unwrap();
        let second = target.invoke(&case("a", "hi")).await.unwrap();
        assert_eq!(first.text, "call-1");
        assert_eq!(second.text, "call-1", "second invoke must replay");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "inner called exactly once");

        let third = target.invoke(&case("b", "different input")).await.unwrap();
        assert_eq!(third.text, "call-2", "different input is a different key");
    }

    #[tokio::test]
    async fn replay_mode_fails_on_miss_and_hits_after_record() {
        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("cache.db");

        let (replay, calls) = cached(
            model_identity(),
            Store::open(&store_path).unwrap(),
            CacheMode::Replay,
        );
        let err = replay.invoke(&case("case-7", "hi")).await.unwrap_err();
        assert!(err.to_string().contains("case-7"), "got: {err}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "replay must never call live"
        );

        let (auto, _) = cached(
            model_identity(),
            Store::open(&store_path).unwrap(),
            CacheMode::Auto,
        );
        auto.invoke(&case("case-7", "hi")).await.unwrap();

        let hit = replay.invoke(&case("case-7", "hi")).await.unwrap();
        assert_eq!(hit.text, "call-1");
    }

    #[tokio::test]
    async fn live_mode_always_calls_and_refreshes_the_recording() {
        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("cache.db");
        let (live, calls) = cached(
            model_identity(),
            Store::open(&store_path).unwrap(),
            CacheMode::Live,
        );

        live.invoke(&case("a", "hi")).await.unwrap();
        let second = live.invoke(&case("a", "hi")).await.unwrap();
        assert_eq!(second.text, "call-2");
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let (replay, _) = cached(
            model_identity(),
            Store::open(&store_path).unwrap(),
            CacheMode::Replay,
        );
        let replayed = replay.invoke(&case("a", "hi")).await.unwrap();
        assert_eq!(replayed.text, "call-2", "live must refresh the recording");
    }

    #[tokio::test]
    async fn uncacheable_targets_pass_through_in_every_mode() {
        for mode in [CacheMode::Auto, CacheMode::Replay, CacheMode::Live] {
            let dir = tempfile::tempdir().unwrap();
            let (target, calls) = cached(None, temp_store(&dir), mode);
            target.invoke(&case("a", "hi")).await.unwrap();
            let second = target.invoke(&case("a", "hi")).await.unwrap();
            assert_eq!(second.text, "call-2", "no caching without an identity");
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        }
    }

    /// Read the single stored canonical request straight from the DB file, for
    /// byte-level cache-key pinning.
    fn stored_request(path: &std::path::Path) -> String {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.query_row("SELECT request FROM llm_cache", [], |r| r.get(0))
            .unwrap()
    }

    #[tokio::test]
    async fn trial_zero_cache_key_bytes_are_unchanged() {
        // HARD back-compat: with no trial scope active (trial 0), the canonical
        // request must be byte-identical to the pre-trials shape, so committed
        // cassettes keep replaying. This literal is the pin.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.db");
        let (target, _) = cached(
            model_identity(),
            Store::open(&path).unwrap(),
            CacheMode::Auto,
        );
        target.invoke(&case("a", "hi")).await.unwrap();
        assert_eq!(
            stored_request(&path),
            r#"{"identity":{"model":"m1","type":"test"},"input":"hi"}"#,
            "trial 0 must not add a trial salt to the canonical request"
        );
    }

    #[tokio::test]
    async fn higher_trials_rekey_the_cache() {
        use evalcore_core::engine::with_trial;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.db");

        // Record trial 0.
        let (t0, _) = cached(
            model_identity(),
            Store::open(&path).unwrap(),
            CacheMode::Auto,
        );
        t0.invoke(&case("a", "hi")).await.unwrap();

        // Trial 1 re-keys, so replay-mode MISSES the trial-0 recording.
        let (replay, _) = cached(
            model_identity(),
            Store::open(&path).unwrap(),
            CacheMode::Replay,
        );
        let err = with_trial(1, replay.invoke(&case("a", "hi")))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cache miss"), "got: {err}");

        // Record and replay at trial 2; it stays deterministic under replay.
        let (t2, calls2) = cached(
            model_identity(),
            Store::open(&path).unwrap(),
            CacheMode::Auto,
        );
        let first = with_trial(2, t2.invoke(&case("a", "hi"))).await.unwrap();
        let second = with_trial(2, t2.invoke(&case("a", "hi"))).await.unwrap();
        assert_eq!(first.text, second.text, "trial 2 replays its own recording");
        assert_eq!(calls2.load(Ordering::SeqCst), 1, "trial 2 recorded once");

        // The stored trial-2 request carries the `trial` salt (sorted keys:
        // identity, input, trial).
        let conn = rusqlite::Connection::open(&path).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM llm_cache WHERE request = ?1",
                [r#"{"identity":{"model":"m1","type":"test"},"input":"hi","trial":2}"#],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "trial 2 request is salted with the trial index");
    }

    #[tokio::test]
    async fn different_identities_do_not_share_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("cache.db");
        let (m1, _) = cached(
            Some(serde_json::json!({"model": "m1"})),
            Store::open(&store_path).unwrap(),
            CacheMode::Auto,
        );
        let (m2, m2_calls) = cached(
            Some(serde_json::json!({"model": "m2"})),
            Store::open(&store_path).unwrap(),
            CacheMode::Auto,
        );

        m1.invoke(&case("a", "hi")).await.unwrap();
        m2.invoke(&case("a", "hi")).await.unwrap();
        assert_eq!(
            m2_calls.load(Ordering::SeqCst),
            1,
            "same input under another model must miss"
        );
    }
}
