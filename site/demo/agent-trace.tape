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

Set Shell "bash"
Set FontSize 16
Set Width 1000
Set Height 620
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 38ms
Set Framerate 24
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

Sleep 600ms
Type "# Grade a recorded agent: the final answer AND the trajectory, one suite."
Enter
Sleep 500ms
Type "# No invocation, no network — EvalCore ingests the trace and asserts."
Enter
Sleep 500ms
Type "evalcore run examples/agent-trace/evals.yaml"
Enter
Sleep 3.5s
