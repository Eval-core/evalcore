# evalcore-config

> Parent: [root CLAUDE.md](../../CLAUDE.md) · architecture: [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md) · map: [MAP.md](../../MAP.md)

The `evals.yaml` schema: serde types, parsing, validation. **Pure data — this crate must never grow I/O (beyond reading the file), network calls, or engine logic**, and it depends on no other workspace crate.

## Rules

- Every new user-facing feature starts here as a config surface, designed YAML-first (write the YAML you want users to type, then derive the types).
- Tagged enums (`TargetConfig`, `ScorerConfig`) use `tag = "type"` + `rename_all = "kebab-case"`. Note: serde does not support `deny_unknown_fields` on internally tagged enums — keep it on structs only.
- Optional fields get `#[serde(default)]`; behavioral defaults get a named `default_*` fn so the default is greppable and documented.
- Secrets are never inline in YAML: reference environment variables by name (`api_key_env`), resolved later by factories in other crates.
- Structural validation (non-empty sections, ranges) lives in `validate()`, which runs on every parse. Validation that needs work (compiling a regex, resolving an env var) belongs in the factories in `evalcore-scorers` / `evalcore-core`, not here.
- Every schema change ships with a parse test in the same PR — including one negative test (what should be rejected).
