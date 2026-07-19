# Rendered with: vhs site/demo/quickstart.tape   (run from the repo root)
#
# Hero terminal demo for the EvalCore docs site. Every frame is REAL output
# from the release binary (target/release/evalcore) run against the shipped,
# offline examples — no network, no API keys, no faked output.
#
# Prereq:  cargo build --release -p evalcore   (repo root)
# Install VHS: https://github.com/charmbracelet/vhs

Output site/public/casts/quickstart.gif
Output site/public/casts/quickstart.webm

Source site/demo/theme.tape
Set Height 880

# Pin `evalcore` to the freshly built release binary and use a clean prompt,
# without showing the plumbing on screen. Aliasing the absolute path (rather
# than prepending PATH) guarantees a missing build errors loudly instead of
# silently falling through to a stale globally-installed evalcore.
Hide
Type 'export PS1="$ "'
Enter
Type 'alias evalcore="$PWD/target/release/evalcore"'
Enter
Type "clear"
Enter
Show

# A beat of stillness so loop-watchers catch the start of the session.
Sleep 800ms
Type "# One YAML suite: a support bot, graded against the policy it must cite."
Enter
Sleep 500ms
Type "# No SDK, no network, no API keys."
Enter
Sleep 700ms
Type "evalcore run examples/quickstart/evals.yaml"
Enter
Sleep 4s

Type "# Every case green, the compliance gate holds, and CI gates on exit 0."
Enter
Sleep 700ms
Type "echo $?"
Enter
Sleep 4s
