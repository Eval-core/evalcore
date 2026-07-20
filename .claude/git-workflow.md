---
# EvalCore git profile. Read by the global github-workflow skill (Step 0).
# Single source of truth for this repo's git specifics.
repos:
  - name: Eval-core/evalcore
    remote: https://github.com/Eval-core/evalcore.git
    visibility: public
    default_branch: main
collaboration: team
maintainers: [abhishekmanyam, kuladeepmantri]
main_shared: true          # co-maintained: branch + PR + review, no direct push, no force-push
branching: always-branch
commit_trailer: |
  Co-Authored-By: Claude <noreply@anthropic.com>
  # the harness may also append a per-session "Claude-Session: <url>" line
scopes: [config, core, scorers, report, store, serve, cli, site, docs, design, ci]
humanizer_paths:
  ship: [README.md, CHANGELOG.md, docs/**, site/**, SECURITY.md, CONTRIBUTING.md]
  internal: [wiki/**, .claude/**]
private_split:
  path: wiki
  remote: https://github.com/Eval-core/brain.git   # private "brain": positioning, competitive analysis, roadmap, decisions
  rule: gitignored from this repo; pushed only to brain; nothing from it enters a public tracked file; internal pages skip the humanizer; direct-to-main on brain is fine
release:
  license: Apache-2.0
  changelog: CHANGELOG.md
  versioning: semver
verified: 2026-07-20
---

# EvalCore git profile

Two repos, one wall:

- **`Eval-core/evalcore`** (public) is co-maintained, so `main` is shared: every
  change goes through a branch and a PR that a co-maintainer reviews. No direct
  pushes to main, no force-push on shared history.
- **`Eval-core/brain`** (private) is the `wiki/` knowledge base. It lives at
  `wiki/` inside this checkout, is gitignored here, and is pushed only to `brain`.
  Nothing from it ever lands in a public tracked file. It is a curated vault, so
  direct commits to its `main` are fine, and its internal pages skip the humanizer.

Before any git action in this repo, the global github-workflow skill loads this
file (Step 0) and applies it. To change these conventions, edit this file.
