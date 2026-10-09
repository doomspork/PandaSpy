/**
 * What a printer row should say about itself, and where it sorts.
 *
 * Pure functions over {@link PrinterView}: no Svelte, no i18n, no clock. The
 * list puts whatever needs the user first, then running jobs soonest-done
 * first, then idle printers, then ones that are not connected — the user's
 * own order (Settings → Printers) breaks every tie.
 */

import type { PrinterView } from './ipc';

/** The short state label shown next to a printer's name (localised via `chip`). */
export type Chip = 'run' | 'pause' | 'warn' | 'fail' | 'auth' | 'idle' | 'done' | 'wait' | 'off';

export interface Condition {
	chip: Chip;
	/** Sort bucket: 0 needs attention, 1 running, 2 idle/finished, 3 not connected. */
	rank: 0 | 1 | 2 | 3;
	/** A job is on the plate (printing, preparing or paused). */
	busy: boolean;
}

const BUSY = new Set(['printing', 'preparing', 'paused']);

// Failures the supervisor will retry on its own read as "offline"; the rest
// stop the session until the user does something, so they rank first.
const RETRIED_FAILURES = new Set(['unreachable', 'connection-closed']);

export function condition(printer: PrinterView): Condition {
	const { connection, state } = printer;

	if (connection.status === 'failed') {
		if (connection.reason === 'wrong-access-code') return { chip: 'auth', rank: 0, busy: false };
		if (connection.reason !== null && RETRIED_FAILURES.has(connection.reason)) {
			return { chip: 'off', rank: 3, busy: false };
		}
		return { chip: 'fail', rank: 0, busy: false };
	}
	if (connection.status === 'disconnected') return { chip: 'off', rank: 3, busy: false };
	if (connection.status !== 'connected' || !state) return { chip: 'wait', rank: 3, busy: false };

	const busy = state.status !== null && BUSY.has(state.status);
	const hasIssue =
		state.printError !== null || state.hms.some((entry) => entry.severity !== 'info');

	if (state.status === 'failed') return { chip: 'fail', rank: 0, busy };
	if (hasIssue) return { chip: 'warn', rank: 0, busy };
	if (state.status === 'paused') return { chip: 'pause', rank: 0, busy };
	if (busy) return { chip: 'run', rank: 1, busy };
	if (state.status === 'finished') return { chip: 'done', rank: 2, busy };
	return { chip: 'idle', rank: 2, busy };
}

/** Attention first, then running (soonest done first), idle, offline; stable otherwise. */
export function sortByAttention(printers: PrinterView[]): PrinterView[] {
	return printers
		.map((printer, index) => ({ printer, index, cond: condition(printer) }))
		.sort((a, b) => {
			if (a.cond.rank !== b.cond.rank) return a.cond.rank - b.cond.rank;
			if (a.cond.rank === 1) {
				const ra = a.printer.state?.remainingSecs ?? Infinity;
				const rb = b.printer.state?.remainingSecs ?? Infinity;
				if (ra !== rb) return ra - rb;
			}
			return a.index - b.index;
		})
		.map((entry) => entry.printer);
}

export interface FleetSummary {
	total: number;
	printing: number;
	attention: number;
	/** Seconds until the soonest running job finishes, when any reports one (paused jobs excluded). */
	nextDoneSecs: number | null;
}

export function summarize(printers: PrinterView[]): FleetSummary {
	let printing = 0;
	let attention = 0;
	let nextDoneSecs: number | null = null;
	for (const printer of printers) {
		const cond = condition(printer);
		if (cond.rank === 0) attention++;
		if (cond.busy) {
			printing++;
			// A paused job's estimate is frozen, so it cannot be "next done".
			if (printer.state?.status === 'paused') continue;
			const remaining = printer.state?.remainingSecs ?? null;
			if (remaining !== null && (nextDoneSecs === null || remaining < nextDoneSecs)) {
				nextDoneSecs = remaining;
			}
		}
	}
	return { total: printers.length, printing, attention, nextDoneSecs };
}

/**
 * How far through the job, 0–1. Layers first — they are what the printer is
 * physically doing, where `mc_percent` is the slicer's time estimate — and
 * the reported percentage only when the layer count is missing.
 */
export function jobFraction(printer: PrinterView): number | null {
	const state = printer.state;
	if (!state) return null;
	if (state.layer !== null && state.totalLayers !== null && state.totalLayers > 0) {
		return Math.max(0, Math.min(1, state.layer / state.totalLayers));
	}
	if (state.progressPercent !== null) return Math.max(0, Math.min(1, state.progressPercent / 100));
	return null;
}

export function displayName(printer: PrinterView): string {
	return printer.nickname || printer.deviceName || printer.model || printer.serial;
}
