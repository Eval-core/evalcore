# EvalCore quickstart demo — charmbracelet VHS tape.
#
# This is the source of truth for the hero terminal demo. VHS is NOT installed
# in the docs build; render it locally and commit the resulting GIF:
#
#     vhs site/demo/quickstart.tape
#
# It runs the shipped, keyless quickstart suite twice: a live record run, then
# the instant $0 replay from the committed cassette.
#
# Install VHS: https://github.com/charmbracelet/vhs

Output quickstart.gif

Set FontSize 18
Set Width 1200
Set Height 640
Set Padding 24
Set Theme "Dracula"
Set TypingSpeed 60ms
Set PlaybackSpeed 1.0

# --- First run: record live ---
Type "# First run: record every LLM call into a local cassette"
Enter
Sleep 800ms
Type "evalcore run evals.yaml --cache live"
Enter
Sleep 2s

# (Output, illustrative)
# PASS refund-1 (612ms)
# PASS refund-2 (588ms)
#
# 2 passed, 0 failed, 2 total · 1240 tokens · $0.0006

Sleep 2s
Enter

# --- Second run: replay, offline, $0 ---
Type "# Second run: replay from the cassette — offline, deterministic, $0"
Enter
Sleep 800ms
Type "evalcore run evals.yaml --cache replay"
Enter
Sleep 1500ms

# (Output, illustrative)
# PASS refund-1 (0ms)
# PASS refund-2 (0ms)
#
# 2 passed, 0 failed, 2 total · 1240 tokens · $0.0000

Sleep 3s
