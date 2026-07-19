# Rendered with: vhs site/demo/trials.tape   (run from the repo root)
#
# Statistical-trials demo for the EvalCore docs site. Every frame is REAL output
# from the release binary (target/release/evalcore) run against the shipped,
# offline site/demo/trials suite — a deterministic shell target, no network,
# no API keys, no faked output.
#
# Prereq:  cargo build --release -p evalcore   (repo root)
# Install VHS: https://github.com/charmbracelet/vhs

Output site/public/casts/trials.gif
Output site/public/casts/trials.webm

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
Type "# Run every case 3 times; the [k/N trials] tag folds the verdicts."
Enter
Sleep 500ms
Type "evalcore run site/demo/trials/evals.yaml"
Enter
Sleep 3.5s
