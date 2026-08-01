/**
 * Pure, locale-neutral formatting for values that are numbers, not words.
 * Anything with grammar (plurals, word order) belongs in Fluent instead — see
 * `locales/`. Nothing here touches the network, the clock, or i18n.
 */

/** `5820` seconds -> `"1h 37m"`; `45` -> `"45m"`. `null`/negative/non-finite -> `null`. */
export function formatDuration(totalSecs: number | null): string | null {
	if (totalSecs === null || !Number.isFinite(totalSecs) || totalSecs < 0) return null;
	const totalMinutes = Math.round(totalSecs / 60);
	const hours = Math.floor(totalMinutes / 60);
	const minutes = totalMinutes % 60;
	return hours > 0 ? `${hours}h ${minutes}m` : `${minutes}m`;
}

/** Round to the nearest whole unit for display. `null`/non-finite -> `null`. */
export function roundOrNull(value: number | null): number | null {
	return value === null || !Number.isFinite(value) ? null : Math.round(value);
}

/** `"00AE42FF"` (RGBA hex, no `#`) -> CSS `#00AE42FF`. `null` when absent or malformed. */
export function toCssColor(rgbaHex: string | null): string | null {
	if (!rgbaHex || !/^[0-9a-fA-F]{8}$/.test(rgbaHex)) return null;
	return `#${rgbaHex}`;
}
