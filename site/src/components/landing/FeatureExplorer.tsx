// Interactive replacement for the old nine-card text grid: one tab per
// capability, each explained by a small purpose-built visual plus two
// sentences and the config/flag that turns it on. Styles live in landing.css
// (the island shares the page's design tokens).
import { useId, useState } from 'react';
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
	return (
		<svg viewBox="0 0 320 190" className="fx-svg" aria-hidden="true">
			<g className="fx-muted-stroke">
				<rect x="14" y="24" width="86" height="34" rx="8" />
				<text x="57" y="45" className="fx-label" textAnchor="middle">
					request
				</text>
				<path d="M100 41 h34" markerEnd="url(#fx-arr)" />
				<rect x="136" y="24" width="62" height="34" rx="8" />
				<text x="167" y="45" className="fx-label" textAnchor="middle">
					hash
				</text>
				<path d="M198 41 h34" markerEnd="url(#fx-arr)" />
			</g>
			<g>
				<rect x="234" y="14" width="74" height="54" rx="10" className="fx-accent-stroke" />
				<circle cx="256" cy="41" r="9" className="fx-reel" />
				<circle cx="286" cy="41" r="9" className="fx-reel" />
				<path d="M256 50 h30" className="fx-accent-stroke" />
			</g>
			<g className="fx-muted-stroke">
				<path d="M271 68 v22" markerEnd="url(#fx-arr)" />
				<rect x="180" y="96" width="128" height="34" rx="8" />
				<text x="244" y="117" className="fx-label" textAnchor="middle">
					replay: 0ms · $0
				</text>
			</g>
			<text x="14" y="120" className="fx-note">
				same request hash →
			</text>
			<text x="14" y="138" className="fx-note">
				same recorded answer,
			</text>
			<text x="14" y="156" className="fx-note">
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
		<div className="fx-trials" aria-hidden="true">
			{rows.map(([id, trials]) => {
				const passed = trials.filter(Boolean).length;
				const flaky = passed > 0 && passed < trials.length;
				return (
					<div key={id} className="fx-trial-row">
						<code>{id}</code>
						<span className="fx-dots">
							{trials.map((t, i) => (
								<i key={i} className={t ? 'is-pass' : 'is-fail'} />
							))}
						</span>
						<span className={`fx-tag ${flaky ? 'is-flaky' : passed === 3 ? 'is-pass' : 'is-fail'}`}>
							{flaky ? `flaky [${passed}/3]` : `[${passed}/3 trials]`}
						</span>
					</div>
				);
			})}
		</div>
	);
}

function MatrixVisual() {
	return (
		<div className="fx-matrix" aria-hidden="true">
			<div className="fx-matrix-row fx-matrix-head">
				<span>case</span>
				<span>gpt</span>
				<span>claude</span>
				<span>winner</span>
			</div>
			{[
				['refund-1', true, true, 'tie'],
				['refund-2', false, true, 'claude'],
				['policy-1', true, true, 'tie'],
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
				<span>ties 2</span>
			</div>
		</div>
	);
}

function TrajectoryVisual() {
	return (
		<svg viewBox="0 0 400 210" className="fx-svg" aria-hidden="true">
			<g className="fx-muted-stroke">
				<path d="M116 58 h30" markerEnd="url(#fx-arr2)" />
				<path d="M252 58 h30" markerEnd="url(#fx-arr2)" />
			</g>
			{[
				['search_kb', 18],
				['get_policy', 154],
				['refund', 290],
			].map(([name, x]) => (
				<g key={String(name)}>
					<rect x={Number(x)} y="38" width="94" height="40" rx="10" className="fx-node is-ok" />
					<text x={Number(x) + 47} y="62" className="fx-label" textAnchor="middle">
						{name}
					</text>
				</g>
			))}
			<g className="fx-check">
				<path d="M24 120 l7 7 12 -14" />
				<text x="58" y="128" className="fx-note">
					must_call order held
				</text>
			</g>
			<g className="fx-cross">
				<path d="M26 152 l12 12 M38 152 l-12 12" />
				<text x="58" y="162" className="fx-note">
					must_not_call: delete_user
				</text>
			</g>
			<g className="fx-check">
				<path d="M24 188 l7 7 12 -14" />
				<text x="58" y="196" className="fx-note">
					3 steps ≤ max_steps 6
				</text>
			</g>
			<defs>
				<marker id="fx-arr2" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
					<path d="M0 0 L10 5 L0 10 z" className="fx-arrow" />
				</marker>
			</defs>
		</svg>
	);
}

function GatesVisual() {
	return (
		<div className="fx-gates" aria-hidden="true">
			<div className="fx-gate">
				<div className="fx-gate-head">
					<code>pass_rate ≥ 0.95</code>
					<span className="fx-tag is-pass">holds</span>
				</div>
				<div className="fx-meter">
					<i className="fx-meter-fill" style={{ width: '100%' }} />
					<i className="fx-meter-mark" style={{ left: '95%' }} />
				</div>
			</div>
			<div className="fx-gate">
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
			<div className="fx-report-doc">
				<div className="fx-report-title">
					<span />
					<b className="t-pass">98%</b>
				</div>
				<div className="fx-report-row is-pass" />
				<div className="fx-report-row is-pass" />
				<div className="fx-report-row is-fail" />
				<div className="fx-report-row is-pass" />
			</div>
			<div className="fx-report-side">
				<span className="fx-note">pass rate, last 12 runs</span>
				<svg viewBox="0 0 120 40" className="fx-spark">
					<polyline points="0,26 12,22 24,24 36,18 48,20 60,12 72,16 84,10 96,12 108,6 120,8" />
				</svg>
				<code>evalcore serve</code>
			</div>
		</div>
	);
}

function CostVisual() {
	return (
		<div className="fx-cost" aria-hidden="true">
			<div className="fx-cost-row">
				<span className="fx-note">this run</span>
				<b>2,202 tokens · $0.0038</b>
			</div>
			<div className="fx-meter">
				<i className="fx-meter-fill" style={{ width: '38%' }} />
				<i className="fx-meter-mark" style={{ left: '80%' }} />
			</div>
			<div className="fx-cost-row">
				<span className="fx-note">budget_usd 0.01 stops scheduling at the cap</span>
			</div>
			<div className="fx-cost-replay">
				<span className="t-pass">replayed run</span>
				<span className="t-dim">same totals, virtual: $0 spent</span>
			</div>
		</div>
	);
}

function ProtocolsVisual() {
	return (
		<svg viewBox="0 0 320 190" className="fx-svg" aria-hidden="true">
			{/* Elbow connectors, staggered lanes, detached 4px from every box. */}
			<g className="fx-muted-stroke">
				<path d="M69 44 V90 H112" />
				<path d="M251 44 V90 H208" />
				<path d="M79 146 V106 H112" />
				<path d="M241 146 V106 H208" />
				<rect x="14" y="10" width="110" height="30" rx="8" />
				<rect x="196" y="10" width="110" height="30" rx="8" />
				<rect x="14" y="150" width="130" height="30" rx="8" />
				<rect x="176" y="150" width="130" height="30" rx="8" />
			</g>
			<rect x="118" y="74" width="84" height="46" rx="10" className="fx-accent-stroke" />
			<text x="160" y="94" className="fx-label" textAnchor="middle">
				engine
			</text>
			<text x="160" y="110" className="fx-note" textAnchor="middle">
				Rust, hidden
			</text>
			<text x="69" y="29" className="fx-label" textAnchor="middle">
				HTTP target
			</text>
			<text x="251" y="29" className="fx-label" textAnchor="middle">
				shell target
			</text>
			<text x="79" y="169" className="fx-label" textAnchor="middle">
				scorer: JSON stdio
			</text>
			<text x="241" y="169" className="fx-label" textAnchor="middle">
				OTel / OpenInference
			</text>
		</svg>
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
	const baseId = useId();
	const feature = FEATURES[active];

	return (
		<div className="fx">
			<div className="fx-tabs" role="tablist" aria-label="EvalCore capabilities">
				{FEATURES.map((f, i) => (
					<button
						key={f.id}
						role="tab"
						id={`${baseId}-tab-${f.id}`}
						aria-selected={i === active}
						aria-controls={`${baseId}-panel-${f.id}`}
						tabIndex={i === active ? 0 : -1}
						className={i === active ? 'fx-tab is-active' : 'fx-tab'}
						onClick={() => setActive(i)}
						onKeyDown={(e) => {
							if (e.key === 'ArrowRight') setActive((active + 1) % FEATURES.length);
							if (e.key === 'ArrowLeft') setActive((active + FEATURES.length - 1) % FEATURES.length);
						}}
					>
						{f.label}
					</button>
				))}
			</div>
			<div
				className="fx-panel"
				role="tabpanel"
				id={`${baseId}-panel-${feature.id}`}
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
				<div className="fx-panel-visual" key={feature.id}>
					{feature.visual}
				</div>
			</div>
		</div>
	);
}
