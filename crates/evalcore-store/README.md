# evalcore-store

Internal component of **[EvalCore](https://crates.io/crates/evalcore)** — snapshot testing for AI behavior: a single-binary, config-first eval runner for LLM apps and agents.

You probably want the `evalcore` CLI itself:

```sh
cargo install evalcore
```

Depend on this crate directly only if you're embedding EvalCore's engine in your own tool. APIs are pre-1.0 and move with the CLI's needs; see the [repository](https://github.com/eval-core/evalcore) for architecture docs.
