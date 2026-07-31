/**
 * Frontend half of PandaSpy's internationalisation.
 *
 * The Rust half (`src-tauri/src/i18n.rs`) reads the very same `.ftl` files from
 * `locales/`. There is no duplicated string table and no build step that copies
 * one into the other — both sides glob the same directory, so a key that exists
 * for the tray menu exists for the window too.
 *
 * Adding a language means adding `locales/<tag>/` and nothing else. The glob
 * below discovers it; `cargo xtask locale-check` makes sure it is complete.
 */

import { FluentBundle, FluentResource, type FluentVariable } from '@fluent/bundle';
import { negotiateLanguages } from '@fluent/langneg';

/**
 * Every `.ftl` in the repository's `locales/` directory, inlined at build time.
 *
 * The leading slash makes this relative to the Vite root (the repository root),
 * not to this file — which is what lets the frontend read a directory that
 * lives outside `src/`.
 */
const sources = import.meta.glob('/locales/*/*.ftl', {
	query: '?raw',
	import: 'default',
	eager: true
}) as Record<string, string>;

/** The locale everything falls back to, and the one CI treats as the reference. */
export const DEFAULT_LOCALE = 'en-US';

function buildBundles(): Map<string, FluentBundle> {
	const bundles = new Map<string, FluentBundle>();

	for (const [path, source] of Object.entries(sources)) {
		// '/locales/en-US/app.ftl' -> 'en-US'
		const tag = path.split('/')[2];
		if (!tag) continue;

		let bundle = bundles.get(tag);
		if (!bundle) {
			// `useIsolating` inserts Unicode directional isolate marks around
			// placeables. Correct for bidirectional text, but it puts invisible
			// characters into strings that tests and `===` comparisons then trip
			// over. Turn it back on when PandaSpy ships an RTL locale.
			bundle = new FluentBundle(tag, { useIsolating: false });
			bundles.set(tag, bundle);
		}

		const errors = bundle.addResource(new FluentResource(source));
		for (const error of errors) {
			// A malformed .ftl is a build-time mistake. Surfacing it loudly beats
			// rendering the raw message id and wondering why.
			console.error(`[i18n] ${path}: ${error.message}`);
		}
	}

	return bundles;
}

const bundles = buildBundles();

/** Locale tags that actually have translations, sorted. */
export const availableLocales: readonly string[] = [...bundles.keys()].sort();

let activeLocale = DEFAULT_LOCALE;

/**
 * Pick the best available locale for a set of user preferences.
 *
 * Uses proper BCP-47 negotiation rather than string equality, so a user whose
 * OS reports `pl` or `pl-PL-u-ca-gregory` still gets Polish.
 */
export function negotiate(requested: readonly string[]): string {
	const [best] = negotiateLanguages([...requested], [...availableLocales], {
		defaultLocale: DEFAULT_LOCALE,
		strategy: 'lookup'
	});
	return best ?? DEFAULT_LOCALE;
}

/** Switch the active locale and update the document's `lang` attribute. */
export function setLocale(tag: string): void {
	activeLocale = bundles.has(tag) ? tag : DEFAULT_LOCALE;
	if (typeof document !== 'undefined') {
		document.documentElement.lang = activeLocale;
	}
}

export function getLocale(): string {
	return activeLocale;
}

/**
 * Resolve a message id in the active locale, falling back to {@link DEFAULT_LOCALE}.
 *
 * Returns the id itself if the key exists in no locale at all. That is
 * deliberately ugly: a bare `tray-quit` in the UI is impossible to miss,
 * whereas an empty string looks like a rendering bug.
 */
export function t(id: string, args?: Record<string, FluentVariable>): string {
	for (const tag of [activeLocale, DEFAULT_LOCALE]) {
		const bundle = bundles.get(tag);
		const message = bundle?.getMessage(id);
		if (bundle && message?.value) {
			const errors: Error[] = [];
			const formatted = bundle.formatPattern(message.value, args, errors);
			for (const error of errors) {
				console.error(`[i18n] ${tag}/${id}: ${error.message}`);
			}
			return formatted;
		}
	}

	console.error(`[i18n] missing key: ${id}`);
	return id;
}
