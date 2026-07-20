// Interactive replacement for the old nine-card text grid: one tab per
// capability, each explained by a small purpose-built visual plus two
// sentences and the config/flag that turns it on. Styles live in landing.css
// (the island shares the page's design tokens).
import { useEffect, useId, useState } from 'react';
import type { ReactNode } from 'react';

type Feature = {
	id: string;
	label: string;
	title: string;
	body: string;
	chip: string;
	href: string;
	visual: ReactNode;
};

function CassetteVisual() {
	// The pipeline reads left to right into the cassette (the one accent
	// element), then drops to the replay card. Cards are raised surfaces, not
	// bare strokes, so the diagram carries the same weight as the DOM visuals.
	return (
		<svg viewBox="0 0 520 268" className="fx-svg" aria-hidden="true">
			<g>
				<rect x="18" y="34" width="120" height="60" rx="10" className="fx-svg-card" />
				<text x="78" y="61" className="fx-label" textAnchor="middle">
					request
				</text>
				<text x="78" y="79" className="fx-sub" textAnchor="middle">
					canonical JSON
				</text>
				<path d="M142 64 h32" className="fx-flow" markerEnd="url(#fx-arr)" />
				<rect x="178" y="34" width="96" height="60" rx="10" className="fx-svg-card" />
				<text x="226" y="61" className="fx-label" textAnchor="middle">
					hash
				</text>
				<text x="226" y="79" className="fx-sub" textAnchor="middle">
					sha-256
				</text>
				<path d="M278 64 h32" className="fx-flow" markerEnd="url(#fx-arr)" />
			</g>
			<g>
				<rect x="314" y="18" width="186" height="112" rx="14" className="fx-accent-stroke" />
				<rect x="336" y="38" width="142" height="46" rx="23" className="fx-cassette-window" />
				<circle cx="366" cy="61" r="12" className="fx-reel" />
				<circle cx="448" cy="61" r="12" className="fx-reel" />
				<path d="M380 61 h54" className="fx-tape" />
				<text x="407" y="112" className="fx-label" textAnchor="middle">
					cassette.db · sqlite
				</text>
			</g>
			<g>
				<path d="M407 130 v34" className="fx-flow" markerEnd="url(#fx-arr)" />
				<rect x="314" y="172" width="186" height="72" rx="10" className="fx-svg-card" />
				<text x="407" y="203" className="fx-label" textAnchor="middle">
					replay
				</text>
				<text x="407" y="223" className="fx-sub" textAnchor="middle">
					0 ms · $0 · no network
				</text>
			</g>
			<text x="18" y="186" className="fx-note">
				same request hash →
			</text>
			<text x="18" y="206" className="fx-note">
				same recorded answer,
			</text>
			<text x="18" y="226" className="fx-note">
				byte for byte
			</text>
			<defs>
				<marker id="fx-arr" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
					<path d="M0 0 L10 5 L0 10 z" className="fx-arrow" />
				</marker>
			</defs>
		</svg>
	);
}

function TrialsVisual() {
	const rows: [string, boolean[]][] = [
		['refund-1', [true, true, true]],
		['greeting', [false, false, false]],
		['tone-check', [true, false, true]],
	];
	return (
		<div className="fx-trials fx-card" aria-hidden="true">
			<div className="fx-table-head fx-trials-grid">
				<span>case</span>
				<span>trials</span>
				<span>verdict</span>
			</div>
			{rows.map(([id, trials]) => {
				const passed = trials.filter(Boolean).length;
				const flaky = passed > 0 && passed < trials.length;
				return (
					<div key={id} className="fx-trial-row fx-trials-grid">
						<code>{id}</code>
						<span className="fx-dots">
							{trials.map((t, i) => (
								<i key={i} className={t ? 'is-pass' : 'is-fail'} />
							))}
						</span>
						<span className={`fx-tag ${flaky ? 'is-flaky' : passed === 3 ? 'is-pass' : 'is-fail'}`}>
							{flaky ? `flaky ${passed}/3` : passed === 3 ? 'pass 3/3' : 'fail 0/3'}
						</span>
					</div>
				);
			})}
			<div className="fx-table-foot">
				<code>fold: majority</code>
				<span>1 pass · 1 fail · 1 flaky</span>
			</div>
		</div>
	);
}

function MatrixVisual() {
	return (
		<div className="fx-matrix fx-card" aria-hidden="true">
			<div className="fx-matrix-row fx-table-head">
				<span>case</span>
				<span>gpt</span>
				<span>claude</span>
				<span>winner</span>
			</div>
			{[
				['refund-1', true, true, 'tie'],
				['refund-2', false, true, 'claude'],
				['tone-1', false, true, 'claude'],
			].map(([id, a, b, w]) => (
				<div key={String(id)} className="fx-matrix-row">
					<code>{id}</code>
					<span className={a ? 't-pass' : 't-fail'}>{a ? 'PASS' : 'FAIL'}</span>
					<span className={b ? 't-pass' : 't-fail'}>{b ? 'PASS' : 'FAIL'}</span>
					<span className="fx-winner">{w}</span>
				</div>
			))}
			<div className="fx-matrix-row fx-matrix-foot">
				<span>wins</span>
				<span>0</span>
				<span className="t-pass">2</span>
				<span>ties 1</span>
			</div>
			<div className="fx-matrix-row fx-matrix-foot fx-matrix-cost">
				<span>cost</span>
				<span>$0.014</span>
				<span>$0.011</span>
				<span>exit 0</span>
			</div>
		</div>
	);
}

function StepArrow() {
	return (
		<svg viewBox="0 0 24 12" className="fx-trace-arrow" aria-hidden="true">
			<path d="M1 6 h17 M14 1.5 l5.5 4.5 -5.5 4.5" />
		</svg>
	);
}

function TrajectoryVisual() {
	// The recorded tool calls flow across the top; the assertions the
	// trajectory scorer checked read as a verdict list beneath them.
	const checks: ['ok' | 'no', string][] = [
		['ok', 'must_call order held'],
		['no', 'must_not_call: delete_user'],
		['ok', '3 steps ≤ max_steps: 6'],
	];
	return (
		<div className="fx-trace" aria-hidden="true">
			<div className="fx-trace-flow">
				{['search_kb', 'get_policy', 'refund'].map((name, i) => (
					<span key={name} className="fx-trace-step">
						{i > 0 && <StepArrow />}
						<span className="fx-trace-node">
							<i>{i + 1}</i>
							<code>{name}</code>
						</span>
					</span>
				))}
			</div>
			<div className="fx-trace-checks fx-card">
				{checks.map(([kind, text]) => (
					<div key={text} className="fx-trace-check">
						{kind === 'ok' ? (
							<svg viewBox="0 0 24 24" className="fx-glyph is-pass">
								<path d="M5 13l5 5 9-11" />
							</svg>
						) : (
							<svg viewBox="0 0 24 24" className="fx-glyph is-fail">
								<path d="M6 6l12 12 M18 6L6 18" />
							</svg>
						)}
						<code>{text}</code>
					</div>
				))}
			</div>
		</div>
	);
}

function GatesVisual() {
	return (
		<div className="fx-gates" aria-hidden="true">
			<div className="fx-gate fx-card">
				<div className="fx-gate-head">
					<code>pass_rate ≥ 0.95</code>
					<span className="fx-tag is-pass">holds</span>
				</div>
				<div className="fx-meter">
					<i className="fx-meter-fill" style={{ width: '98%' }} />
					<i className="fx-meter-mark" style={{ left: '95%' }} />
				</div>
				<div className="fx-meter-scale">
					<span>0</span>
					<span className="fx-meter-cap" style={{ left: '95%' }}>
						0.95
					</span>
				</div>
			</div>
			<div className="fx-gate fx-card">
				<div className="fx-gate-head">
					<code>--baseline main</code>
					<span className="fx-tag is-pass">no regressions</span>
				</div>
				<ul className="fx-gate-list">
					<li className="t-dim">known-fail tone-2, tolerated</li>
					<li className="t-pass">fixed greeting-3</li>
				</ul>
			</div>
		</div>
	);
}

function ReportsVisual() {
	return (
		<div className="fx-report" aria-hidden="true">
			<div className="fx-report-doc fx-card">
				<div className="fx-report-bar">
					<span className="fx-report-dots">
						<i />
						<i />
						<i />
					</span>
					<code>report.html</code>
				</div>
				<div className="fx-report-body">
					<div className="fx-report-title">
						<span />
						<b className="t-pass">98%</b>
					</div>
					<div className="fx-report-row is-pass" style={{ width: '92%' }} />
					<div className="fx-report-row is-pass" style={{ width: '78%' }} />
					<div className="fx-report-row is-fail" style={{ width: '85%' }} />
					<div className="fx-report-row is-pass" style={{ width: '64%' }} />
				</div>
			</div>
			<div className="fx-report-side">
				<span className="fx-note">pass rate, last 12 runs</span>
				<svg viewBox="0 0 120 44" className="fx-spark">
					<path
						className="fx-spark-area"
						d="M0 26 L12 22 L24 24 L36 18 L48 20 L60 12 L72 16 L84 10 L96 12 L108 6 L120 8 L120 44 L0 44 Z"
					/>
					<polyline points="0,26 12,22 24,24 36,18 48,20 60,12 72,16 84,10 96,12 108,6 120,8" />
					<circle className="fx-spark-dot" cx="120" cy="8" r="3" />
				</svg>
				<code>evalcore serve</code>
			</div>
		</div>
	);
}

function CostVisual() {
	return (
		<div className="fx-cost fx-card" aria-hidden="true">
			<div className="fx-cost-stat">
				<b>$0.0038</b>
				<span>2,202 tokens · this run</span>
			</div>
			<div className="fx-meter">
				<i className="fx-meter-fill is-accent" style={{ width: '38%' }} />
				<i className="fx-meter-mark" style={{ left: '80%' }} />
			</div>
			<div className="fx-meter-scale">
				<span>$0</span>
				<span className="fx-meter-cap" style={{ left: '80%' }}>
					budget_usd 0.01
				</span>
			</div>
			<p className="fx-cost-note">New cases stop scheduling at the cap; the run still exits cleanly.</p>
			<div className="fx-cost-replay">
				<span className="t-pass">replayed run</span>
				<span className="t-dim">same totals · virtual: $0 spent</span>
			</div>
		</div>
	);
}

function ProtocolsVisual() {
	// One lane per extension point: what you write, the wire format between,
	// and what EvalCore calls it. The old version packed four boxes and elbow
	// connectors into 320x190 and the labels ran out of their boxes.
	const lanes: [string, string, string][] = [
		['Your app', 'HTTP or shell', 'target'],
		['Your scorer', 'JSON over stdio', 'scorer'],
		['Your traces', 'OTel / OpenInference', 'trajectory'],
	];
	return (
		<div className="fx-proto" aria-hidden="true">
			{lanes.map(([mine, wire, theirs]) => (
				<div key={wire} className="fx-proto-lane">
					<span className="fx-proto-side">{mine}</span>
					<span className="fx-proto-wire">
						<code>{wire}</code>
					</span>
					<span className="fx-proto-side is-engine">{theirs}</span>
				</div>
			))}
			<p className="fx-proto-note">Any language on the left. Nothing to link, no Rust to write.</p>
		</div>
	);
}

const FEATURES: Feature[] = [
	{
		id: 'replay',
		label: 'Record / replay',
		title: 'Cassettes make evals free and deterministic',
		body: 'Every model call is recorded to a local SQLite cassette, keyed on a hash of the canonical request. Commit it, and CI replays byte-for-byte: no network, no keys, no flaky judges.',
		chip: '--cache replay',
		href: '/guides/record-replay/',
		visual: <CassetteVisual />,
	},
	{
		id: 'trials',
		label: 'Trials',
		title: 'One sample is not a measurement',
		body: 'run.trials repeats every case N times and folds the verdicts with all, majority, or any. Split verdicts are flagged flaky instead of silently passing or failing your build.',
		chip: 'run.trials: 3',
		href: '/guides/trials-and-statistics/',
		visual: <TrialsVisual />,
	},
	{
		id: 'matrix',
		label: 'Compare models',
		title: 'One suite, several targets, one verdict',
		body: 'A matrix run executes the same suite against every listed target in a single invocation: per-case winners, win and tie counts, per-target cost, and one exit code for CI.',
		chip: '--matrix gpt,claude',
		href: '/guides/comparing-models/',
		visual: <MatrixVisual />,
	},
	{
		id: 'agents',
		label: 'Agent traces',
		title: 'Grade the path, not just the answer',
		body: 'Feed recorded OTel or OpenInference traces to a trace target and score the trajectory: which tools ran, in what order, within what step budget, alongside final-answer scorers.',
		chip: 'type: trajectory',
		href: '/guides/agents-and-traces/',
		visual: <TrajectoryVisual />,
	},
	{
		id: 'gates',
		label: 'Gates & baselines',
		title: 'Block regressions, not imperfection',
		body: 'Suite gates set absolute floors like pass_rate or mean_score. Baselines tolerate the failures you have accepted and fail the run only when something that passed starts failing.',
		chip: '--baseline main',
		href: '/guides/gates-and-baselines/',
		visual: <GatesVisual />,
	},
	{
		id: 'reports',
		label: 'Reports & history',
		title: 'Every run is an artifact',
		body: 'One self-contained HTML file per run holds every case, score, and diff. Local history feeds evalcore serve: a read-only viewer with pass-rate trends and run-to-run diffs on localhost.',
		chip: '--html report.html',
		href: '/guides/html-reports/',
		visual: <ReportsVisual />,
	},
	{
		id: 'cost',
		label: 'Cost',
		title: 'Tokens and dollars are part of the result',
		body: 'Declare cost rates and every case reports tokens and dollars; run.budget_usd stops scheduling new cases at the cap. Replayed runs report their recorded totals as virtual cost.',
		chip: 'run.budget_usd: 0.01',
		href: '/guides/cost-and-budgets/',
		visual: <CostVisual />,
	},
	{
		id: 'protocols',
		label: 'Any language',
		title: 'Protocols over SDKs',
		body: 'Targets speak HTTP or shell. Custom scorers speak JSON over stdin/stdout. Judges are any OpenAI-compatible endpoint. Rust is the engine; you never write it, link it, or install it as a library.',
		chip: 'type: shell',
		href: '/getting-started/core-concepts/',
		visual: <ProtocolsVisual />,
	},
];

export default function FeatureExplorer() {
	const [active, setActive] = useState(0);
	// The rail is vertical on wide screens and lies down horizontally under
	// 60rem; keep aria-orientation honest so keyboard users are told the right
	// axis. Starts vertical (the SSR/wide default) and corrects on mount.
	const [orientation, setOrientation] = useState<'vertical' | 'horizontal'>('vertical');
	useEffect(() => {
		const mq = window.matchMedia('(max-width: 60rem)');
		const sync = () => setOrientation(mq.matches ? 'horizontal' : 'vertical');
		sync();
		mq.addEventListener('change', sync);
		return () => mq.removeEventListener('change', sync);
	}, []);
	const baseId = useId();
	const feature = FEATURES[active];
	// One panel is rendered and its content swaps, so every tab points at this
	// single stable id — the old per-feature id left 6 of 7 tabs referencing a
	// panel that was not in the DOM.
	const panelId = `${baseId}-panel`;

	return (
		// not-content opts the island out of Starlight's prose sibling margins,
		// which otherwise add 1rem above every div/i inside the visuals.
		<div className="fx not-content">
			<div
				className="fx-rail"
				role="tablist"
				aria-orientation={orientation}
				aria-label="EvalCore capabilities"
			>
				{FEATURES.map((f, i) => (
					<button
						key={f.id}
						role="tab"
						id={`${baseId}-tab-${f.id}`}
						aria-selected={i === active}
						aria-controls={panelId}
						tabIndex={i === active ? 0 : -1}
						className={i === active ? 'fx-tab is-active' : 'fx-tab'}
						onClick={() => setActive(i)}
						onKeyDown={(e) => {
							// The rail is vertical on wide screens and horizontal when it
							// wraps, so both axes drive selection.
							const next =
								e.key === 'ArrowDown' || e.key === 'ArrowRight'
									? 1
									: e.key === 'ArrowUp' || e.key === 'ArrowLeft'
										? -1
										: 0;
							if (!next) return;
							e.preventDefault();
							const target = (active + next + FEATURES.length) % FEATURES.length;
							setActive(target);
							document.getElementById(`${baseId}-tab-${FEATURES[target].id}`)?.focus();
						}}
					>
						{f.label}
					</button>
				))}
			</div>
			<div
				className="fx-panel"
				role="tabpanel"
				id={panelId}
				aria-labelledby={`${baseId}-tab-${feature.id}`}
			>
				<div className="fx-panel-copy">
					<h3>{feature.title}</h3>
					<p>{feature.body}</p>
					<div className="fx-panel-meta">
						<code className="fx-chip">{feature.chip}</code>
						<a href={feature.href}>
							Read the guide
							<svg aria-hidden="true" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
								<path d="M9 5l7 7-7 7" />
							</svg>
						</a>
					</div>
				</div>
				<div className="fx-stage">
					<div className="fx-panel-visual" key={feature.id}>
						{feature.visual}
					</div>
				</div>
			</div>
		</div>
	);
}
