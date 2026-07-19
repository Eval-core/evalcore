# Rendered with: vhs site/demo/matrix.tape   (run from the repo root)
#
# Model-comparison demo for the EvalCore docs site. Every frame is REAL output
# from the release binary run against the shipped, offline site/demo/matrix
# suite — two deterministic shell targets, no network, no keys.
#
# Prereq:  cargo build --release -p evalcore   (repo root)
# Install VHS: https://github.com/charmbracelet/vhs

Output site/public/casts/matrix.gif
Output site/public/casts/matrix.webm

Source site/demo/theme.tape
Set Height 1440

Hide
Type 'export PS1="$ "'
Enter
Type 'alias evalcore="$PWD/target/release/evalcore"'
Enter
Type "clear"
Enter
Show

Sleep 800ms
Type "# Same suite, two targets. Which one actually holds up?"
Enter
Sleep 700ms
Type "evalcore run site/demo/matrix/evals.yaml --matrix good,bad"
Enter
Sleep 5s

Type "# Per-case winners, a wins line, and one exit code for CI."
Enter
Sleep 4.5s
