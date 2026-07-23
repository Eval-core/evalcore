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
main_shared: false         # equal co-owners: either maintainer pushes direct to main; no review gate; no force-push
branching: direct-to-main  # branch/PR only when a maintainer wants discussion first
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

- **`Eval-core/evalcore`** (public) is owned equally by its two maintainers.
  Either pushes direct to main; neither needs the other's review or permission.
  Branches and PRs exist only for changes one of them wants discussed first.
  No force-push on main.
- **`Eval-core/brain`** (private) is the `wiki/` knowledge base. It lives at
  `wiki/` inside this checkout, is gitignored here, and is pushed only to `brain`.
  Nothing from it ever lands in a public tracked file. It is a curated vault, so
  direct commits to its `main` are fine, and its internal pages skip the humanizer.

Before any git action in this repo, the global github-workflow skill loads this
file (Step 0) and applies it. To change these conventions, edit this file.
