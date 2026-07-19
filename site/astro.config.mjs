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
				'Snapshot testing for AI behavior: a single-binary, config-first eval runner for LLM apps and agents.',
			favicon: '/favicon.svg',
			// Self-hosted fonts (no external requests). The site title set in Inter
			// is the brand — there is no logo. Order matters: fonts first, then the
			// theme that references them.
			customCss: [
				'@fontsource-variable/inter',
				'@fontsource/jetbrains-mono/400.css',
				'@fontsource/jetbrains-mono/500.css',
				'@fontsource/jetbrains-mono/700.css',
				'./src/styles/custom.css',
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
					],
				},
				{
					label: 'Guides',
					items: [
						{ label: 'Running in CI', slug: 'guides/running-in-ci' },
						{ label: 'Record / replay', slug: 'guides/record-replay' },
						{ label: 'Evaluating REST APIs', slug: 'guides/evaluating-rest-apis' },
						{ label: 'Agents and traces', slug: 'guides/agents-and-traces' },
						{ label: 'RAG evaluation', slug: 'guides/rag-evaluation' },
						{ label: 'LLM-as-judge', slug: 'guides/llm-as-judge' },
						{ label: 'Semantic similarity', slug: 'guides/semantic-similarity' },
						{ label: 'Cost and budgets', slug: 'guides/cost-and-budgets' },
						{ label: 'Custom scorers', slug: 'guides/custom-scorers' },
						{ label: 'Gates and baselines', slug: 'guides/gates-and-baselines' },
						{ label: 'Trials and statistics', slug: 'guides/trials-and-statistics' },
						{ label: 'Classification', slug: 'guides/classification' },
						{ label: 'Comparing models', slug: 'guides/comparing-models' },
						{ label: 'Run history and serve', slug: 'guides/run-history-and-serve' },
						{ label: 'HTML reports', slug: 'guides/html-reports' },
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
