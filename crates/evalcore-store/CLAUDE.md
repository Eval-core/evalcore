# evalcore-store

> Parent: [root CLAUDE.md](../../CLAUDE.md) · architecture: [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md) · map: [MAP.md](../../MAP.md)

Local SQLite storage. Today: the **record/replay cache** (`Store`, `CacheMode`, `CachedTarget`). Next: run history for `--baseline` diffing. Depends only on `evalcore-core`.

## Cache semantics (user-facing contract — change with care)

- Key = SHA-256 of the canonical request JSON: `{"identity": <target.cache_identity()>, "input": <case input>}`. `serde_json::Value` serializes with sorted keys — **never enable serde_json's `preserve_order` feature**, it would silently invalidate every cache on disk.
- Anything that changes what would be sent (model, url, future params) must be inside `cache_identity()`. Secrets (API keys) must never be — they don't change the response semantics and must not be persisted, even hashed alongside requests.
- Modes: `Auto` (replay hit / record miss), `Replay` (miss = case failure, never calls live), `Live` (always call, refresh recording). Targets with `cache_identity() == None` bypass the cache in every mode.
- Replayed outputs are returned verbatim, including recorded `latency_ms`.
- The cache file (`.evalcore/cache.db`) is a project artifact like a VCR cassette: committing it gives CI free, deterministic `--cache replay` runs.

## Rules

- Schema changes: `llm_cache` is on-disk format. Additive changes only; anything else needs a versioned migration story first.
- Keep storage synchronous behind the mutex — lookups are microseconds next to LLM calls; don't add async plumbing without a measured reason.
- A corrupt cache entry is an error telling the user to delete the cache file, never a silent live call (that would un-determinize a replay run).
- Tests use `tempfile` and the counting fake target; never a real network.
