// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
	// Deployed to GitHub Pages under the repo path. When a custom domain lands,
	// flip `base` to '/' (and point `site` at the custom domain).
	site: 'https://eval-core.github.io',
	base: '/evalcore',
	integrations: [
		starlight({
			title: 'EvalCore',
			description:
				'The eval engine for AI systems: measure, compare, and regression-gate LLM apps and agents with one config-first binary.',
			favicon: '/favicon.png',
			// Site-wide footer strip (links every repo artifact the site never
			// surfaced). Wraps Starlight's default footer so docs pages keep
			// their prev/next pagination.
			components: {
				Hero: './src/components/Hero.astro',
				Footer: './src/components/Footer.astro',
			},
			// The mark: orbit-and-check symbol next to the wordmark. Light/dark
			// variants because the near-black strokes would vanish on the dark bg.
			logo: {
				light: './src/assets/logo-light.png',
				dark: './src/assets/logo-dark.png',
				alt: 'EvalCore',
			},
			// Self-hosted fonts (no external requests). Order matters: fonts first,
			// then the design system. The five style files load in dependency
			// order — tokens define the variables every later file reads.
			customCss: [
				'@fontsource-variable/inter',
				'@fontsource/jetbrains-mono/400.css',
				'@fontsource/jetbrains-mono/500.css',
				'@fontsource/jetbrains-mono/700.css',
				'./src/styles/tokens.css',
				'./src/styles/base.css',
				'./src/styles/chrome.css',
				'./src/styles/landing.css',
				'./src/styles/components.css',
			],
			social: [
				{
					icon: 'github',
					label: 'GitHub',
					href: 'https://github.com/eval-core/evalcore',
				},
			],
			// Code blocks: quiet monochrome surfaces, hairline border, one 6px
			// radius, no frame shadow, and the site's own mono/sans families.
			expressiveCode: {
				themes: ['github-light', 'github-dark'],
				styleOverrides: {
					borderRadius: 'var(--radius)',
					borderColor: 'var(--sl-color-hairline)',
					codeBackground: 'var(--code-bg)',
					codeFontFamily: 'var(--sl-font-mono)',
					uiFontFamily: 'var(--sl-font)',
					frames: {
						shadowColor: 'transparent',
						editorTabBarBackground: 'var(--code-bg)',
						editorActiveTabBackground: 'var(--code-bg)',
						editorTabBarBorderBottomColor: 'var(--sl-color-hairline)',
						// The active file-tab indicator defaults to the theme accent,
						// which leaked orange into code chrome (D15). Force it to the
						// text color so the tab reads active without the accent.
						editorActiveTabIndicatorTopColor: 'var(--sl-color-text)',
						editorActiveTabBorderColor: 'var(--sl-color-hairline)',
						terminalBackground: 'var(--code-bg)',
						terminalTitlebarBackground: 'var(--code-bg)',
						terminalTitlebarBorderBottomColor: 'var(--sl-color-hairline)',
					},
				},
			},
			sidebar: [
				{
					label: 'Getting started',
					items: [
						{ label: 'Installation', slug: 'getting-started/installation' },
						{ label: 'Quickstart', slug: 'getting-started/quickstart' },
						{ label: 'Core concepts', slug: 'getting-started/core-concepts' },
						{ label: 'What teams use it for', slug: 'getting-started/what-teams-use-it-for' },
					],
				},
				{
					label: 'Workflow & CI',
					items: [
						{ label: 'Running in CI', slug: 'guides/running-in-ci' },
						{ label: 'Record / replay', slug: 'guides/record-replay' },
						{ label: 'Gates and baselines', slug: 'guides/gates-and-baselines' },
						{ label: 'Run history and serve', slug: 'guides/run-history-and-serve' },
						{ label: 'HTML reports', slug: 'guides/html-reports' },
					],
				},
				{
					label: 'Targets & data',
					items: [
						{ label: 'Evaluating REST APIs', slug: 'guides/evaluating-rest-apis' },
						{ label: 'Agents and traces', slug: 'guides/agents-and-traces' },
						{ label: 'RAG evaluation', slug: 'guides/rag-evaluation' },
					],
				},
				{
					label: 'Scorers',
					items: [
						{ label: 'LLM-as-judge', slug: 'guides/llm-as-judge' },
						{ label: 'Semantic similarity', slug: 'guides/semantic-similarity' },
						{ label: 'Custom scorers', slug: 'guides/custom-scorers' },
						{ label: 'Classification', slug: 'guides/classification' },
					],
				},
				{
					label: 'Analysis',
					items: [
						{ label: 'Trials and statistics', slug: 'guides/trials-and-statistics' },
						{ label: 'Comparing models', slug: 'guides/comparing-models' },
						{ label: 'Cost and budgets', slug: 'guides/cost-and-budgets' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'Configuration', slug: 'reference/configuration' },
						{ label: 'CLI', slug: 'reference/cli' },
						{ label: 'Subprocess protocol', slug: 'reference/subprocess-protocol' },
						{ label: 'Trajectory format', slug: 'reference/trajectory-format' },
						{ label: 'Cache and determinism', slug: 'reference/cache-and-determinism' },
					],
				},
				{
					label: 'FAQ',
					items: [{ label: 'FAQ', slug: 'faq' }],
				},
			],
		}),
	],
});
