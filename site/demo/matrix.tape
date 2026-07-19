# Rendered with: vhs site/demo/matrix.tape   (run from the repo root)
#
# Model-comparison demo for the EvalCore docs site. Every frame is REAL output
# from the release binary (target/release/evalcore) run against the shipped,
# offline site/demo/matrix suite — two deterministic shell targets, no network,
# no API keys, no faked output.
#
# Prereq:  cargo build --release -p evalcore   (repo root)
# Install VHS: https://github.com/charmbracelet/vhs

Output site/public/casts/matrix.gif
Output site/public/casts/matrix.webm

# Rendered at 2x pixel density for retina sharpness: the same ~100-column grid
# (FontSize 32 on a 2000x1600 canvas) at twice the resolution. Taller than the
# other casts so the full per-arm blocks and the comparison table fit unscrolled.
Set Shell "bash"
Set FontSize 32
Set Width 2000
Set Height 1600
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
Type "# One suite, two targets: good answers refunds, bad deflects."
Enter
Sleep 500ms
Type "evalcore run site/demo/matrix/evals.yaml --matrix good,bad"
Enter
Sleep 3.5s
