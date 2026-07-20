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
				Head: './src/components/Head.astro',
				SiteTitle: './src/components/SiteTitle.astro',
				ThemeProvider: './src/components/ThemeProvider.astro',
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
				'@fontsource-variable/manrope',
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
			// Code blocks are ALWAYS dark, in both themes: a dark instrument panel
			// on a light page reads as the thing being operated, and syntax colors
			// keep their full contrast. All chrome colors are fixed dark values
			// for the same reason — these never follow the page theme.
			expressiveCode: {
				themes: ['github-dark'],
				useDarkModeMediaQuery: false,
				styleOverrides: {
					borderRadius: 'var(--radius)',
					borderColor: '#26262c',
					codeBackground: '#0e0e11',
					codeFontFamily: 'var(--sl-font-mono)',
					uiFontFamily: 'var(--sl-font)',
					frames: {
						shadowColor: 'transparent',
						editorTabBarBackground: '#0e0e11',
						editorActiveTabBackground: '#0e0e11',
						editorTabBarBorderBottomColor: '#26262c',
						// The active file-tab indicator defaults to the theme accent;
						// force it to the text color so the tab reads active without
						// turning the code chrome accent-colored.
						editorActiveTabIndicatorTopColor: '#e4e4e7',
						editorActiveTabBorderColor: '#26262c',
						editorTabBarInactiveForeground: '#9d9da6',
						editorActiveTabForeground: '#e4e4e7',
						terminalBackground: '#0e0e11',
						terminalTitlebarBackground: '#0e0e11',
						terminalTitlebarBorderBottomColor: '#26262c',
						terminalTitlebarForeground: '#9d9da6',
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
						// Labels match each page's own title so the sidebar, breadcrumb,
						// and H1 read the same.
						{ label: 'Configuration reference', slug: 'reference/configuration' },
						{ label: 'CLI reference', slug: 'reference/cli' },
						{ label: 'Subprocess scorer protocol', slug: 'reference/subprocess-protocol' },
						{ label: 'Trajectory format', slug: 'reference/trajectory-format' },
						{ label: 'Cache and determinism', slug: 'reference/cache-and-determinism' },
					],
				},
				// A single-item group rendered as "FAQ > FAQ"; a plain top-level link.
				{ label: 'FAQ', slug: 'faq' },
			],
		}),
	],
});
