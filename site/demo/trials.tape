# Rendered with: vhs site/demo/trials.tape   (run from the repo root)
#
# Statistical-trials demo for the EvalCore docs site. Every frame is REAL
# output from the release binary run against the shipped, offline
# site/demo/trials suite — a deterministic shell target, no network, no keys.
#
# Prereq:  cargo build --release -p evalcore   (repo root)
# Install VHS: https://github.com/charmbracelet/vhs

Output site/public/casts/trials.gif
Output site/public/casts/trials.webm

Source site/demo/theme.tape
Set Height 720

Hide
Type 'export PS1="$ "'
Enter
Type 'alias evalcore="$PWD/target/release/evalcore"'
Enter
Type "clear"
Enter
Show

Sleep 800ms
Type "# One sample of a stochastic model is not a measurement."
Enter
Sleep 500ms
Type "# run.trials: 3 runs every case three times and folds the verdicts."
Enter
Sleep 700ms
Type "evalcore run site/demo/trials/evals.yaml"
Enter
Sleep 4s

Type "# [k/N trials] per case; split verdicts would be counted as flaky."
Enter
Sleep 4s
