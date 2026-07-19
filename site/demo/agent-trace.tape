# Rendered with: vhs site/demo/agent-trace.tape   (run from the repo root)
#
# Trajectory-eval demo for the EvalCore docs site. Every frame is REAL output
# from the release binary run against the shipped, offline agent-trace
# example — no network, no API keys, no faked output.
#
# Prereq:  cargo build --release -p evalcore   (repo root)
# Install VHS: https://github.com/charmbracelet/vhs

Output site/public/casts/agent-trace.gif
Output site/public/casts/agent-trace.webm

Source site/demo/theme.tape
Set Height 800

Hide
Type 'export PS1="$ "'
Enter
Type 'alias evalcore="$PWD/target/release/evalcore"'
Enter
Type "clear"
Enter
Show

Sleep 800ms
Type "# An agent is more than its final answer: grade the recorded path too."
Enter
Sleep 500ms
Type "# Native trajectories and OTel/OpenInference traces, same suite."
Enter
Sleep 700ms
Type "evalcore run examples/agent-trace/evals.yaml"
Enter
Sleep 4s

Type "# must_call ordering, step budgets, and final-answer scorers together."
Enter
Sleep 4s
