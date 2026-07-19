// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import react from '@astrojs/react';

// https://astro.build/config
export default defineConfig({
	// Served from the custom domain evalcore.cc at the root, so base is '/'.
	// (The public/CNAME file pins the domain across GitHub Pages deploys.)
	site: 'https://evalcore.cc',
	base: '/',
	integrations: [
		// React powers the interactive landing islands (feature explorer,
		// animated terminal). Docs pages stay zero-JS Astro.
		react(),
		starlight({
			title: 'EvalCore',
			description:
				'Snapshot testing for AI behavior: measure, compare, and regression-gate LLM apps and agents with one config-first binary.',
			favicon: '/favicon.svg',
			// Every visible piece of Starlight chrome is overridden so the docs
			// read as this site, not as a themed default: lockup, theme toggle,
			// GitHub button, compact pagination, split footer (mega on the
			// landing, slim on docs), and the custom landing hero.
			components: {
				SiteTitle: './src/components/SiteTitle.astro',
				ThemeSelect: './src/components/ThemeToggle.astro',
				SocialIcons: './src/components/GitHubButton.astro',
				Pagination: './src/components/Pagination.astro',
				Hero: './src/components/Hero.astro',
				Footer: './src/components/Footer.astro',
			},
			// Self-hosted fonts (no external requests). Order matters: fonts first,
			// then the design system. The five style files load in dependency
			// order — tokens define the variables every later file reads.
			customCss: [
				'@fontsource-variable/geist',
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
			// Code blocks: quiet monochrome surfaces, hairline border, no frame
			// shadow, no fake traffic lights (chrome.css hides the terminal dots;
			// CastFrame owns showpiece windows). The site's own mono/sans families.
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
						// The active file-tab indicator defaults to the theme accent;
						// force it to the text color so the tab reads active without
						// turning the code chrome accent-colored.
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
