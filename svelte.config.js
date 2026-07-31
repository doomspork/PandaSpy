import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
export default {
	preprocess: vitePreprocess(),
	kit: {
		// Tauri serves the built frontend from the filesystem, so there is no
		// Node server to render on. adapter-static emits a plain SPA that
		// `src-tauri/tauri.conf.json` points at as `frontendDist`.
		// No `fallback`: every route is prerendered (see src/routes/+layout.ts),
		// so index.html is a real prerendered page rather than an empty shell.
		// `strict: true` turns "someone added a route that cannot be
		// prerendered" into a build error instead of a 404 at runtime.
		adapter: adapter({
			pages: 'build',
			assets: 'build',
			precompress: false,
			strict: true
		})
	}
};
