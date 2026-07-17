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

# Rendered at 2x pixel density for retina sharpness: the same ~100x28 column
# grid (FontSize 32 on a 2000x1400 canvas) at twice the resolution.
Set Shell "bash"
Set FontSize 32
Set Width 2000
Set Height 1400
Set Padding 40
Set Theme "Catppuccin Mocha"
Set TypingSpeed 38ms
Set Framerate 20
Set PlaybackSpeed 1.0

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
Type "# A suite is one YAML file: a target, a dataset, and scorers."
Enter
Sleep 400ms
Type "cat examples/quickstart/evals.yaml"
Enter
Sleep 3s

Type "# Run it — the shell (cat) target needs no network and no keys."
Enter
Sleep 400ms
Type "evalcore run examples/quickstart/evals.yaml"
Enter
Sleep 3s

Type "# Same command, applied to a recorded agent: grade the answer AND the path."
Enter
Sleep 400ms
Type "evalcore run examples/agent-trace/evals.yaml"
Enter
Sleep 3.5s

Type "# Exit 0 = everything passed. Gate CI on it."
Enter
Sleep 400ms
Type "echo $?"
Enter
Sleep 2.5s
