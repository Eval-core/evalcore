//! Pure HTML rendering for the viewer's own pages: the run-history listing
//! (table + pass-rate sparkline), and the small 404/400 pages. Run detail and
//! diff pages are rendered by `evalcore-report`, not here — this module only
//! owns the listing chrome. Every DB-derived string is escaped with
//! [`evalcore_report::html_escape`], the same rule the report uses.

use evalcore_report::html_escape;
use evalcore_store::RunMeta;

/// Newest count of runs the sparkline plots.
const SPARKLINE_RUNS: usize = 50;

/// Render the run-history listing: a newest-first table (id, time, config,
/// target, passed/failed/total, cost) with a per-row link to `/run/:id` and,
/// when an older run of the same target exists, a `diff` link to it. A
/// server-computed pass-rate sparkline over the newest [`SPARKLINE_RUNS`] runs
/// sits above the table. A corrupt row renders as an inline error entry — it
/// never drops the row or the page.
pub fn index_page(runs: &[RunMeta]) -> String {
    let mut out = String::new();
    page_head(&mut out, "EvalCore runs");
    out.push_str("<h1>EvalCore runs</h1>\n");

    if runs.is_empty() {
        out.push_str(
            "<p class=\"muted\">No runs recorded yet. Run <code>evalcore run</code> \
             (history is on by default) and refresh.</p>\n",
        );
        page_foot(&mut out);
        return out;
    }

    out.push_str(&sparkline(runs));

    out.push_str("<table>\n<thead><tr>");
    out.push_str("<th>Run</th><th>Recorded</th><th>Config</th><th>Target</th>");
    out.push_str("<th class=\"num\">Passed</th><th class=\"num\">Failed</th>");
    out.push_str("<th class=\"num\">Total</th><th class=\"num\">Cost</th><th>Diff</th>");
    out.push_str("</tr></thead>\n<tbody>\n");
    for (i, run) in runs.iter().enumerate() {
        push_row(&mut out, runs, i, run);
    }
    out.push_str("</tbody>\n</table>\n");
    page_foot(&mut out);
    out
}

/// One table row. Counts come from the parsed summary; a corrupt row shows an
/// error entry spanning the count columns instead.
fn push_row(out: &mut String, runs: &[RunMeta], i: usize, run: &RunMeta) {
    out.push_str(&format!(
        "<tr><td><a href=\"/run/{id}\">#{id}</a></td><td class=\"muted\">{when}</td>\
         <td>{config}</td><td>{target}</td>",
        id = run.id,
        when = html_escape(&run.created_at),
        config = html_escape(&run.config),
        target = html_escape(&run.target),
    ));
    match &run.summary {
        Ok(summary) => {
            let cost = summary
                .total_cost_usd()
                .map(|c| format!("${c:.4}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "<td class=\"num pass\">{}</td><td class=\"num fail\">{}</td>\
                 <td class=\"num\">{}</td><td class=\"num\">{cost}</td>",
                summary.passed(),
                summary.failed(),
                summary.total(),
            ));
        }
        Err(message) => {
            out.push_str(&format!(
                "<td class=\"error\" colspan=\"4\">{}</td>",
                html_escape(message)
            ));
        }
    }
    // "diff vs previous same-target run": the nearest older row (larger index,
    // since rows are newest-first) sharing this target.
    match previous_same_target(runs, i) {
        Some(prev) => out.push_str(&format!(
            "<td><a href=\"/diff?a={a}&amp;b={b}\">vs #{a}</a></td>",
            a = prev.id,
            b = run.id,
        )),
        None => out.push_str("<td class=\"muted\">—</td>"),
    }
    out.push_str("</tr>\n");
}

/// The nearest older run (higher index in the newest-first list) with the same
/// target, or `None` when this is the first run of its target.
fn previous_same_target(runs: &[RunMeta], i: usize) -> Option<&RunMeta> {
    runs[i + 1..].iter().find(|r| r.target == runs[i].target)
}

/// A pass rate in `[0, 1]` for one run: `passed / total`. Corrupt and empty
/// runs return `None` — they carry no data, and plotting them as `0.0` would
/// make "unreadable row" indistinguishable from "everything failed".
fn pass_rate(run: &RunMeta) -> Option<f64> {
    match &run.summary {
        Ok(summary) if summary.total() > 0 => {
            Some(summary.passed() as f64 / summary.total() as f64)
        }
        _ => None,
    }
}

/// A pure inline-SVG pass-rate sparkline over the newest [`SPARKLINE_RUNS`]
/// runs, plotted oldest→newest left to right. No axes, no text — a compact
/// trend glyph. Deterministic: identical runs render identical bytes.
fn sparkline(runs: &[RunMeta]) -> String {
    // Newest-first input; take the newest N (skipping corrupt/empty rows —
    // they carry no rate) and reverse to chronological order.
    let mut points: Vec<f64> = runs
        .iter()
        .take(SPARKLINE_RUNS)
        .filter_map(pass_rate)
        .collect::<Vec<_>>();
    points.reverse();
    if points.len() < 2 {
        return String::new();
    }
    const W: f64 = 480.0;
    const H: f64 = 48.0;
    const PAD: f64 = 4.0;
    let n = points.len();
    let step = (W - 2.0 * PAD) / (n as f64 - 1.0);
    let coords: Vec<String> = points
        .iter()
        .enumerate()
        .map(|(i, &rate)| {
            let x = PAD + step * i as f64;
            // rate 1.0 at the top (y = PAD), 0.0 at the bottom (y = H - PAD).
            let y = PAD + (1.0 - rate) * (H - 2.0 * PAD);
            format!("{x:.1},{y:.1}")
        })
        .collect();
    format!(
        "<svg class=\"spark\" viewBox=\"0 0 {W:.0} {H:.0}\" width=\"{W:.0}\" height=\"{H:.0}\" \
         role=\"img\" aria-label=\"pass rate over the last {n} runs\">\
         <polyline fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" points=\"{}\"/></svg>\n",
        coords.join(" "),
    )
}

/// A minimal 404 page carrying a plain, escaped message.
pub fn not_found_page(message: &str) -> String {
    simple_page("Not found", "Not found", message)
}

/// A minimal 400 page carrying a plain, escaped message (bad `/diff` ids).
pub fn bad_request_page(message: &str) -> String {
    simple_page("Bad request", "Bad request", message)
}

/// A minimal 500 page carrying a plain, escaped message (store read failures,
/// corrupt summary rows) — distinct from [`bad_request_page`] so a server-side
/// failure is never labeled as the client's fault.
pub fn server_error_page(message: &str) -> String {
    simple_page("Server error", "Server error", message)
}

/// A one-line-message page with a link back to the listing.
fn simple_page(title: &str, heading: &str, message: &str) -> String {
    let mut out = String::new();
    page_head(&mut out, title);
    out.push_str(&format!("<h1>{}</h1>\n", html_escape(heading)));
    out.push_str(&format!(
        "<p class=\"error\">{}</p>\n",
        html_escape(message)
    ));
    out.push_str("<p><a href=\"/\">&larr; all runs</a></p>\n");
    page_foot(&mut out);
    out
}

/// Document prelude through the opening `<main>`. Self-contained: inline CSS,
/// no external requests, no scripts. Title is a static string (never
/// DB-derived), so it needs no escaping.
fn page_head(out: &mut String, title: &str) {
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str(&format!("<title>{title}</title>\n"));
    out.push_str("<style>\n");
    out.push_str(STYLE);
    out.push_str("</style>\n</head>\n<body>\n<main>\n");
}

fn page_foot(out: &mut String) {
    out.push_str("</main>\n</body>\n</html>\n");
}

/// Minimal theme-aware stylesheet, kin to the report's but scoped to the
/// listing. Inlined; no external resources.
const STYLE: &str = "\
:root{--bg:#ffffff;--fg:#1c1e21;--muted:#6b7280;--border:#e5e7eb;--panel:#f9fafb;\
--pass:#137333;--fail:#c5221f;--code:#f3f4f6;--link:#1a56db;}\
@media (prefers-color-scheme:dark){:root{--bg:#16181c;--fg:#e6e6e6;--muted:#9aa0a6;\
--border:#2c2f36;--panel:#1e2127;--pass:#81c995;--fail:#f28b82;--code:#232733;--link:#8ab4f8;}}\
*{box-sizing:border-box;}\
body{margin:0;background:var(--bg);color:var(--fg);\
font:14px/1.5 -apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;}\
main{max-width:960px;margin:0 auto;padding:24px 16px 64px;}\
h1{font-size:20px;margin:0 0 12px;}\
a{color:var(--link);text-decoration:none;}a:hover{text-decoration:underline;}\
table{width:100%;border-collapse:collapse;margin:12px 0;}\
th,td{text-align:left;padding:5px 8px;border-bottom:1px solid var(--border);vertical-align:top;}\
th{font-size:11px;text-transform:uppercase;letter-spacing:.04em;color:var(--muted);font-weight:600;}\
.num{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap;}\
.pass{color:var(--pass);}.fail{color:var(--fail);}\
.muted{color:var(--muted);}\
.error{color:var(--fail);font-weight:600;}\
code{background:var(--code);border-radius:4px;padding:1px 5px;font-size:12.5px;}\
svg.spark{display:block;color:var(--pass);margin:4px 0 8px;max-width:100%;}\
";
