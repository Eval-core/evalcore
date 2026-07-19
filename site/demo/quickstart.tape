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

# Rendered at 2x pixel density for retina sharpness. Canvas is sized to the real
# output so there is no dead space, and the theme background matches the site's
# --code-bg (#151517) so the recording blends into its frame instead of clashing.
Set Shell "bash"
Set FontSize 30
Set Width 1600
Set Height 680
Set Padding 44
Set Theme { "background": "#151517", "foreground": "#e6e6e6", "cursor": "#e6e6e6", "selection": "#2b3a55", "black": "#151517", "red": "#f85149", "green": "#3fb950", "yellow": "#d29922", "blue": "#58a6ff", "magenta": "#bc8cff", "cyan": "#39c5cf", "white": "#e6e6e6", "brightBlack": "#6e7681", "brightRed": "#ff7b72", "brightGreen": "#56d364", "brightYellow": "#e3b341", "brightBlue": "#79c0ff", "brightMagenta": "#d2a8ff", "brightCyan": "#56d4dd", "brightWhite": "#f0f6fc" }
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
Type "# A support bot, graded against the policy each answer must cite."
Enter
Sleep 400ms
Type "# Offline: the shell target needs no model, no network, no keys."
Enter
Sleep 600ms
Type "evalcore run examples/quickstart/evals.yaml"
Enter
Sleep 4s

Type "# 4 grounded answers pass; the compliance gate holds."
Enter
Sleep 400ms
Type "# Exit 0 = green. Gate CI on it."
Enter
Sleep 600ms
Type "echo $?"
Enter
Sleep 3.5s
