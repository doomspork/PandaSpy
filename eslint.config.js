import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';

import svelteConfig from './svelte.config.js';

export default defineConfig([
	// Build output and the Rust side. `target/` in particular holds generated
	// JavaScript that would take longer to lint than the entire app.
	globalIgnores(['build/', '.svelte-kit/', 'src-tauri/', 'target/', 'node_modules/']),

	js.configs.recommended,
	ts.configs.recommended,
	svelte.configs.recommended,

	{
		languageOptions: {
			globals: { ...globals.browser, ...globals.node }
		}
	},
	{
		files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
		languageOptions: {
			parserOptions: {
				parser: ts.parser,
				svelteConfig
			}
		}
	}
]);
