//! Terminal presentation for the CLI: capability detection, the styled verdict
//! banner, next-step hints, and the interactive progress display.
//!
//! Every terminal, environment, and clock read lives here — `evalcore-report`
//! stays a pure `&RunSummary -> String` and is handed a plain-data [`Style`].
//! Nothing in this module writes to stdout; the report owns stdout, progress and
//! hints own stderr.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use clap::ValueEnum;
use evalcore_core::{BaselineDiff, ProgressSink, RunSummary};
use evalcore_report::Style;

/// `--color` policy. `auto` (the default) colors only an interactive,
/// color-permitting stream.
#[derive(Clone, Copy, ValueEnum)]
pub enum ColorArg {
    Auto,
    Always,
    Never,
}

/// `--progress` policy. `auto` shows the interactive counter only when stderr is
/// a TTY; `never` disables it everywhere.
#[derive(Clone, Copy, ValueEnum)]
pub enum ProgressArg {
    Auto,
    Never,
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

fn term_is_dumb() -> bool {
    std::env::var_os("TERM").is_some_and(|t| t == "dumb")
}

/// Whether color is permitted on a stream, honoring `--color`, then (in `auto`)
/// `NO_COLOR`, `TERM=dumb`, `CLICOLOR_FORCE`, and whether the stream is a TTY.
fn color_enabled(policy: ColorArg, is_tty: bool) -> bool {
    match policy {
        ColorArg::Never => false,
        ColorArg::Always => true,
        ColorArg::Auto => {
            if env_set("NO_COLOR") || term_is_dumb() {
                return false;
            }
            if env_set("CLICOLOR_FORCE") {
                return true;
            }
            is_tty
        }
    }
}

/// Unicode glyphs (verdict mark, spinner) are used only on an interactive, non-
/// dumb terminal; redirected output and dumb terminals stay ASCII.
fn unicode_enabled(is_tty: bool) -> bool {
    is_tty && !term_is_dumb()
}

/// Resolved presentation capabilities for one run.
pub struct Ui {
    /// Style for a Terminal report (and its verdict) written to stdout.
    pub stdout: Style,
    /// Style for stderr text: the verdict/hints when stdout is machine-owned,
    /// plus progress.
    pub stderr: Style,
    /// Interactive progress permitted (stderr is a TTY and `--progress` allows).
    pub progress: bool,
    /// Omit passing cases from the listing (`--quiet`).
    pub quiet: bool,
}

/// Detect capabilities from the two output streams and the CLI flags. The only
/// place the process inspects its terminal.
pub fn resolve(color: ColorArg, progress: ProgressArg, quiet: bool) -> Ui {
    let out_tty = std::io::stdout().is_terminal();
    let err_tty = std::io::stderr().is_terminal();
    Ui {
        stdout: Style {
            color: color_enabled(color, out_tty),
            unicode: unicode_enabled(out_tty),
            quiet,
        },
        stderr: Style {
            color: color_enabled(color, err_tty),
            unicode: unicode_enabled(err_tty),
            quiet: false,
        },
        progress: matches!(progress, ProgressArg::Auto) && err_tty,
        quiet,
    }
}

/// The final, prominent verdict line, emitted after the report. Reflects the
/// real exit-code outcome (the CLI folds cases, gates, and baseline before
/// calling this). `detail` is an optional clause (e.g. `1 regressed, 1 new`).
pub fn verdict(passed: bool, detail: Option<&str>, style: &Style) -> String {
    let word = if passed {
        style.pass("PASSED")
    } else {
        style.fail("FAILED")
    };
    let mark = match (style.unicode, passed) {
        (true, true) => style.pass("✔ "),
        (true, false) => style.fail("✗ "),
        (false, _) => String::new(),
    };
    let tail = detail
        .map(|d| style.muted(&format!(" · {d}")))
        .unwrap_or_default();
    format!("\n{mark}{word}{tail}\n")
}

/// The baseline verdict clause, e.g. `2 regressed, 1 new` — `None` when the
/// baseline gate held.
pub fn baseline_detail(diff: &BaselineDiff) -> Option<String> {
    diff.gate_failed().then(|| {
        format!(
            "{} regressed, {} new",
            diff.regressions.len(),
            diff.new_failing.len()
        )
    })
}

/// Actionable next steps, when one exists — dim lines for stderr. Empty when the
/// run is clean or nothing applies. `config_display` is echoed into the record
/// command; it is the path the user already typed, so it is shown verbatim.
pub fn hints(
    summary: &RunSummary,
    replay: bool,
    config_display: &str,
    style: &Style,
) -> Vec<String> {
    let mut out = Vec::new();
    if replay {
        // A replay-mode cache miss surfaces as a target error naming the miss.
        let missed = summary
            .results
            .iter()
            .filter(|r| r.error.as_deref().is_some_and(|e| e.contains("cache miss")))
            .count();
        if missed > 0 {
            let plural = if missed == 1 { "case" } else { "cases" };
            out.push(style.muted(&format!(
                "hint: {missed} {plural} had no cached reply under --cache replay. \
                 Record with: evalcore run {config_display} --cache auto"
            )));
        }
    }
    out
}

/// A hint to refresh the baseline after a regression is reviewed and accepted.
pub fn baseline_accept_hint(diff: &BaselineDiff, label: &str, style: &Style) -> Option<String> {
    diff.gate_failed().then(|| {
        style.muted(&format!(
            "hint: once you've reviewed the diff, accept the new state with --save-baseline {label}"
        ))
    })
}

/// An interactive stderr progress line: a spinner plus a `k/N` counter that
/// updates in place and erases itself before the report prints. TTY-only. The
/// spinner advances one frame per completed case, so no clock is read; draws are
/// serialized across concurrent case completions.
pub struct Progress {
    total: usize,
    done: AtomicUsize,
    label: String,
    style: Style,
    draw_lock: Mutex<()>,
}

const SPINNER_UNICODE: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const SPINNER_ASCII: [char; 4] = ['|', '/', '-', '\\'];

impl Progress {
    /// Build a progress display and the [`ProgressSink`] that drives it. The sink
    /// goes into `RunOptions`; keep the returned handle to [`clear`](Self::clear)
    /// it before rendering the report.
    pub fn start(total: usize, label: &str, style: &Style) -> (ProgressSink, Arc<Progress>) {
        let progress = Arc::new(Progress {
            total,
            done: AtomicUsize::new(0),
            // A hostile target name must not carry control bytes into stderr.
            label: label.chars().filter(|c| !c.is_control()).collect(),
            style: *style,
            draw_lock: Mutex::new(()),
        });
        let sink_handle = Arc::clone(&progress);
        let sink = ProgressSink::new(move || sink_handle.tick());
        (sink, progress)
    }

    fn tick(&self) {
        // Always record completion; the counter is the source of truth.
        let done = self.done.fetch_add(1, Ordering::SeqCst) + 1;
        // Draw only if no other completing case holds the lock. `tick` runs
        // synchronously inside the engine's async tasks, and the stderr write can
        // block under terminal flow-control (Ctrl-S, a slow pty); skipping a
        // contended frame keeps that stall on at most one worker instead of
        // wedging every worker behind the lock. A dropped frame is invisible —
        // the next tick redraws the current count.
        let Ok(_guard) = self.draw_lock.try_lock() else {
            return;
        };
        let frame = if self.style.unicode {
            SPINNER_UNICODE[done % SPINNER_UNICODE.len()]
        } else {
            SPINNER_ASCII[done % SPINNER_ASCII.len()]
        };
        let body = format!("{frame} {} {done}/{}", self.label, self.total);
        // `\r` returns to column 0, the painted body, then erase-to-end-of-line
        // clears any longer previous frame. Erase-to-EOL is cursor control, not
        // color, so it is correct even under NO_COLOR (progress is TTY-only).
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "\r{}\x1b[K", self.style.muted(&body));
        let _ = err.flush();
    }

    /// Erase the progress line so the report starts on a clean row. Call once,
    /// after the run completes and before printing the report.
    pub fn clear(&self) {
        let _guard = self.draw_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "\r\x1b[K");
        let _ = err.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GLYPHED: Style = Style {
        color: false,
        unicode: true,
        quiet: false,
    };

    #[test]
    fn plain_verdict_is_just_the_word() {
        assert_eq!(verdict(true, None, &Style::plain()), "\nPASSED\n");
        assert_eq!(verdict(false, None, &Style::plain()), "\nFAILED\n");
    }

    #[test]
    fn verdict_appends_a_detail_clause() {
        assert_eq!(
            verdict(false, Some("2 regressed, 1 new"), &Style::plain()),
            "\nFAILED · 2 regressed, 1 new\n"
        );
    }

    #[test]
    fn unicode_verdict_prepends_a_mark_but_keeps_the_word() {
        // The glyph is an accent; the PASS/FAIL word is always present, so color
        // and glyphs are never the sole status signal.
        assert_eq!(verdict(true, None, &GLYPHED), "\n✔ PASSED\n");
        assert_eq!(verdict(false, None, &GLYPHED), "\n✗ FAILED\n");
    }

    #[test]
    fn color_policy_extremes_ignore_the_terminal() {
        assert!(!color_enabled(ColorArg::Never, true), "never is never");
        assert!(
            color_enabled(ColorArg::Always, false),
            "always even off-TTY"
        );
    }

    #[test]
    fn unicode_requires_a_tty() {
        assert!(!unicode_enabled(false), "redirected output stays ASCII");
    }
}
