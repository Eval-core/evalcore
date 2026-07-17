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
			logo: {
				src: './src/assets/logo.svg',
				alt: 'EvalCore',
			},
			favicon: '/favicon.svg',
			customCss: ['./src/styles/custom.css'],
			social: [
				{
					icon: 'github',
					label: 'GitHub',
					href: 'https://github.com/eval-core/evalcore',
				},
			],
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
						{ label: 'LLM-as-judge', slug: 'guides/llm-as-judge' },
						{ label: 'Cost and budgets', slug: 'guides/cost-and-budgets' },
						{ label: 'Custom scorers', slug: 'guides/custom-scorers' },
						{ label: 'Gates and baselines', slug: 'guides/gates-and-baselines' },
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
