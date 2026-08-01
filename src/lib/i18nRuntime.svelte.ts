/**
 * A reactive wrapper around `$lib/i18n`.
 *
 * `i18n.ts` keeps the active locale in a plain module variable, which is
 * exactly right for its own purity but invisible to Svelte's fine-grained
 * reactivity: a `setLocale()` call from the Settings screen would change what
 * `t()` returns without prompting anything already on screen to re-render.
 * This module adds a rune-backed counter that every `t()` call here reads, so
 * switching languages repaints the whole tree immediately — without changing
 * `i18n.ts` itself (its `useIsolating(false)` setup is deliberate and stays
 * untouched).
 */

import type { FluentVariable } from '@fluent/bundle';
import * as i18n from './i18n';

let tick = $state(0);

export const DEFAULT_LOCALE = i18n.DEFAULT_LOCALE;
export const availableLocales = i18n.availableLocales;
export const negotiate = i18n.negotiate;
export const getLocale = i18n.getLocale;

/** Switch the active locale and trigger a re-render of every `t()` caller. */
export function setLocale(tag: string): void {
	i18n.setLocale(tag);
	tick++;
}

/** Reactive `t()` — safe to call from a template or a `$derived` expression. */
export function t(id: string, args?: Record<string, FluentVariable>): string {
	void tick;
	return i18n.t(id, args);
}
