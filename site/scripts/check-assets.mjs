// Post-build guard: every local asset a built page references must exist.
//
// This exists because of a real failure. Astro's content-layer cache (.astro/)
// stores rendered markdown, including the <link> to expressive-code's hashed
// stylesheet. When the expressiveCode config changes, the stylesheet's content
// hash changes, but cached pages keep pointing at the old filename. Those pages
// then ship with a 404'd stylesheet and no code-block styling at all, which is
// invisible in the build log and easy to miss in review.
//
// The build script clears .astro/ first so the cache cannot go stale; this
// script is the backstop that fails loudly if anything else dangles.

import { readdir, readFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { join, relative } from 'node:path';

const dist = new URL('../dist/', import.meta.url).pathname;

/** Every .html file under dist, recursively. */
async function htmlFiles(dir) {
	const out = [];
	for (const entry of await readdir(dir, { withFileTypes: true })) {
		const full = join(dir, entry.name);
		if (entry.isDirectory()) out.push(...(await htmlFiles(full)));
		else if (entry.name.endsWith('.html')) out.push(full);
	}
	return out;
}

// href/src values pointing at a root-relative local file (skip protocol,
// protocol-relative, anchor, and data URIs).
const REF = /(?:href|src)="(\/[^"#?]+\.[a-z0-9]+)(?:[?#][^"]*)?"/gi;

const files = await htmlFiles(dist);
const missing = [];

for (const file of files) {
	const html = await readFile(file, 'utf8');
	for (const [, ref] of html.matchAll(REF)) {
		if (!existsSync(join(dist, ref))) {
			missing.push({ page: relative(dist, file), ref });
		}
	}
}

if (missing.length) {
	console.error(`\nBroken asset references in the build (${missing.length}):\n`);
	for (const { page, ref } of missing) console.error(`  ${page} -> ${ref}`);
	console.error('\nIf these are _astro/ec.*.css, the content cache went stale: rm -rf .astro\n');
	process.exit(1);
}

console.log(`Asset check passed: ${files.length} pages, no dangling local references.`);
