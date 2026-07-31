import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// Set by `tauri dev` when developing against a device on the LAN.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
	plugins: [sveltekit()],

	// Tauri's own output is the interesting part of the terminal; do not wipe it.
	clearScreen: false,

	server: {
		port: 1420,
		// Fail loudly rather than silently moving to 1421 — `tauri.conf.json`
		// hard-codes this port and a silent move produces a blank window.
		strictPort: true,
		host: host || false,
		hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
		watch: {
			// Rust changes are Cargo's business; watching them here just causes
			// spurious frontend reloads during a backend rebuild.
			ignored: ['**/src-tauri/**', '**/target/**']
		}
	},

	// `TAURI_ENV_*` is how the Tauri CLI tells the frontend what it is building
	// for. Everything else stays behind the usual `VITE_` prefix.
	envPrefix: ['VITE_', 'TAURI_ENV_*'],

	build: {
		// Tauri's webview is Edge WebView2 on Windows and WebKit elsewhere.
		// Targeting the oldest each platform ships avoids shipping polyfills to
		// browsers that will never run this code.
		target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
		// `true` means Vite's bundled minifier. Naming `'esbuild'` explicitly
		// would pull in a package Vite 8 no longer depends on.
		minify: !process.env.TAURI_ENV_DEBUG,
		sourcemap: Boolean(process.env.TAURI_ENV_DEBUG)
	}
});
