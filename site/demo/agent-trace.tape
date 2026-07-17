# Rendered with: vhs site/demo/agent-trace.tape   (run from the repo root)
#
# Trajectory-eval demo for the EvalCore docs site. Every frame is REAL output
# from the release binary (target/release/evalcore) run against the shipped,
# offline agent-trace example — no network, no API keys, no faked output.
#
# Prereq:  cargo build --release -p evalcore   (repo root)
# Install VHS: https://github.com/charmbracelet/vhs

Output site/public/casts/agent-trace.gif
Output site/public/casts/agent-trace.webm

# Rendered at 2x pixel density for retina sharpness: the same ~100x28 column
# grid (FontSize 32 on a 2000x1240 canvas) at twice the resolution.
Set Shell "bash"
Set FontSize 32
Set Width 2000
Set Height 1240
Set Padding 40
Set Theme "Catppuccin Mocha"
Set TypingSpeed 38ms
Set Framerate 20
Set PlaybackSpeed 1.0

# Pin `evalcore` to the freshly built release binary and use a clean prompt,
# off-screen. Aliasing the absolute path (rather than prepending PATH)
# guarantees a missing build errors loudly instead of silently falling
# through to a stale globally-installed evalcore.
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
Type "# Grade a recorded agent: the final answer AND the trajectory, one suite."
Enter
Sleep 500ms
Type "# No invocation, no network — EvalCore ingests the trace and asserts."
Enter
Sleep 500ms
Type "evalcore run examples/agent-trace/evals.yaml"
Enter
Sleep 3.5s
