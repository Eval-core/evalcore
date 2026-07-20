# Documentation map

Where to find what, across the repository and beyond.

## For users

- **[evalcore.cc](https://evalcore.cc)** is the documentation site: installation,
  quickstart, guides, and the full `evals.yaml` and CLI references. Its source lives in
  [`site/`](../site/), and doc changes ship in the same PR as the config surface they
  describe.
- **[`examples/`](../examples/)** holds runnable suites. `quickstart/` is the five-minute
  intro (and doubles as the CLI test fixture); `support-rag/` and `claims-triage/` are
  fuller real-world setups with their own READMEs.
- **[`shims/`](../shims/)** wraps Ragas and DeepEval metrics behind the subprocess
  scorer protocol, so existing Python metrics run unmodified.

## For contributors

- **[ARCHITECTURE.md](ARCHITECTURE.md)**: the crate map, dependency rules, and how a run
  flows through the engine. Read this before touching Rust.
- **[trajectory-spec.md](trajectory-spec.md)**: the versioned trajectory-assertion
  format used by trace targets.
- **[`CONTRIBUTING.md`](../CONTRIBUTING.md)**: setup, workflow, and the architecture
  rules contributions must follow.
- Each crate under [`crates/`](../crates/) carries its own README and local rules.

## For designers

- **[`design/`](../design/)**: the design system. `design/README.md` is the entry point;
  `design/philosophy/` holds the decisions every user-facing surface follows, and
  `site/src/styles/tokens.css` is the machine-readable copy.
