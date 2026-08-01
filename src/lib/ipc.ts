/**
 * The typed boundary between the window and Rust.
 *
 * Rust owns all printer state — discovery, connections, parsed status, secrets,
 * pins — and the window is a view of it. This module is the one place the
 * frontend names Tauri commands and events; every type here mirrors a
 * `#[derive(Serialize)]` in `src-tauri/src/view.rs` or `src-tauri/src/commands.rs`,
 * in `camelCase`. Keep the two in step: a field renamed on one side must be
 * renamed on the other, and there is no generated glue to catch a drift.
 *
 * The window NEVER opens a socket to a printer. It calls these commands and
 * listens for these events; that is the whole contract.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

// ─── Connection ──────────────────────────────────────────────────────────────

/** Connection lifecycle, flattened for the UI. */
export interface ConnectionView {
	/** `disconnected` | `connecting` | `handshaking` | `connected` | `failed`. */
	status: string;
	/**
	 * For `failed`: a stable reason key to localise (`wrong-access-code`,
	 * `unreachable`, `tls`, `certificate-changed`, `protocol`,
	 * `connection-closed`). `null` otherwise.
	 */
	reason: string | null;
}

// ─── Live snapshot ───────────────────────────────────────────────────────────

/** Which tray is feeding the extruder. */
export interface ActiveTrayView {
	/** `slot` | `external` | `none` | `unknown`. */
	kind: string;
	unit: number | null;
	slot: number | null;
}

export interface TrayView {
	id: number | null;
	occupied: boolean;
	filamentType: string | null;
	/** RGBA hex, e.g. `"00AE42FF"`, when the tray reported a colour. */
	color: string | null;
	remainPercent: number | null;
}

export interface AmsUnitView {
	id: number | null;
	/** `standard` | `lite` | `pro2` | `ht` | `unknown`. */
	kind: string | null;
	humidity: number | null;
	temperature: number | null;
	trays: TrayView[];
}

/** A health-management event, resolved to text in the active locale by Rust. */
export interface HmsView {
	/** e.g. `0300_0200_0001_0001`. */
	code: string | null;
	/** `fatal` | `serious` | `common` | `info` | `unknown`. */
	severity: string | null;
	/** Localised description, or `null` for an unknown code (show `code` + wiki). */
	text: string | null;
	wikiUrl: string | null;
}

/** A UI-ready snapshot of what a printer is doing. */
export interface PrinterSnapshot {
	/** `idle` | `preparing` | `printing` | `paused` | `finished` | `failed` | `unknown`, or `null`. */
	status: string | null;
	/** Fine sub-stage key (`auto-bed-leveling`, …), when known. */
	stage: string | null;
	progressPercent: number | null;
	remainingSecs: number | null;
	layer: number | null;
	totalLayers: number | null;
	nozzleTemp: number | null;
	nozzleTarget: number | null;
	bedTemp: number | null;
	bedTarget: number | null;
	chamberTemp: number | null;
	taskName: string | null;
	ams: AmsUnitView[];
	activeTray: ActiveTrayView | null;
	hms: HmsView[];
	/** Resolved text for a non-zero print error, when there is one. */
	printError: string | null;
}

/** One printer as the list renders it. */
export interface PrinterView {
	serial: string;
	nickname: string | null;
	model: string | null;
	address: string | null;
	connection: ConnectionView;
	/** Absent until the first report arrives. */
	state: PrinterSnapshot | null;
}

// ─── Discovery ───────────────────────────────────────────────────────────────

/**
 * Why discovery found what it found. PascalCase because it is the discovery
 * crate's own enum serialised verbatim; map it to a Fluent key in the UI. The
 * open string member keeps a future (non_exhaustive) variant assignable without
 * losing autocomplete on the known ones.
 */
export type DiscoveryVerdict =
	| 'Found'
	| 'NoUsableInterface'
	| 'PermissionDenied'
	| 'NoResponse'
	// `Record<never, never>` behaves like `{}` (keeps the literals in autocomplete
	// while allowing any string) without tripping `no-empty-object-type`.
	| (string & Record<never, never>);

export interface DiscoveredView {
	serial: string | null;
	address: string;
	/** Human model name (`X1 Carbon`), never the raw product code. */
	model: string | null;
	name: string | null;
	/** `ssdp` | `subnet-probe` | `manual` | `unknown`. */
	source: string;
	/** `true` when this printer is already in the user's list. */
	alreadyAdded: boolean;
}

export interface DiscoveryReport {
	printers: DiscoveredView[];
	verdict: DiscoveryVerdict;
}

// ─── Settings & diagnostics ──────────────────────────────────────────────────

export interface SettingsView {
	/** `null` means "follow the operating system". */
	locale: string | null;
	launchAtLogin: boolean;
	/** `os-keyring` | `encrypted-file` — where access codes actually live. */
	secretBackend: string;
	/** The platform's name for its keyring (`Keychain`, `Credential Manager`, …). */
	keyringName: string;
}

export interface StudioPrinterView {
	serial: string | null;
	name: string | null;
	address: string | null;
}

export interface Diagnostics {
	version: string;
	activeLocale: string;
	availableLocales: string[];
	secretBackend: string;
}

// ─── Commands ────────────────────────────────────────────────────────────────

/** List every configured printer with its current connection and state. */
export function listPrinters(): Promise<PrinterView[]> {
	return invoke('list_printers');
}

/** Scan the local network for printers (SSDP, subnet-probe fallback). */
export function discoverPrinters(): Promise<DiscoveryReport> {
	return invoke('discover_printers');
}

/** Add a printer and bring its session up. `accessCode` never round-trips back. */
export function addPrinter(args: {
	serial: string;
	address: string;
	accessCode: string;
	nickname?: string | null;
}): Promise<void> {
	return invoke('add_printer', args);
}

/** Remove a printer: stop its session, forget its secret and pin. */
export function removePrinter(serial: string): Promise<void> {
	return invoke('remove_printer', { serial });
}

/** Reorder the printer list to match `serials`, top to bottom. */
export function reorderPrinters(serials: string[]): Promise<void> {
	return invoke('reorder_printers', { serials });
}

/**
 * Respond to a {@link onTrustRequired} prompt. `accept` re-pins the new
 * certificate and reconnects; declining leaves the printer stopped.
 */
export function resolveTrust(serial: string, accept: boolean): Promise<void> {
	return invoke('resolve_trust', { serial, accept });
}

/** Read current settings, including where secrets are stored on this host. */
export function getSettings(): Promise<SettingsView> {
	return invoke('get_settings');
}

/** Persist settings. `locale = null` restores "follow the OS". */
export function setSettings(args: {
	locale: string | null;
	launchAtLogin: boolean;
}): Promise<void> {
	return invoke('set_settings', args);
}

/**
 * Best-effort import of printers already configured in Bambu Studio.
 *
 * Returns `[]` for now — parsing Studio's on-disk list is gated on a recorded
 * fixture (see `commands.rs`), so the affordance is wired but finds nothing yet.
 */
export function importStudio(): Promise<StudioPrinterView[]> {
	return invoke('import_studio');
}

/** What the app can tell you about itself (version, locales, secret backend). */
export function diagnostics(): Promise<Diagnostics> {
	return invoke('diagnostics');
}

// ─── Events ──────────────────────────────────────────────────────────────────

/** Event names, mirroring `crate::app::events`. */
export const EVENTS = {
	printerUpdate: 'printer://update',
	trustRequired: 'printer://trust-required'
} as const;

/**
 * A printer's connection and/or state changed. The payload is the whole
 * refreshed {@link PrinterView} — replace the matching card by `serial`.
 */
export function onPrinterUpdate(handler: (printer: PrinterView) => void): Promise<UnlistenFn> {
	return listen<PrinterView>(EVENTS.printerUpdate, (event) => handler(event.payload));
}

/** A printer's certificate changed and needs a decision (see {@link resolveTrust}). */
export interface TrustRequired {
	serial: string;
	/** Hex fingerprint pinned on first sight. */
	pinned: string;
	/** Hex fingerprint the printer presented now. */
	presented: string;
}

export function onTrustRequired(handler: (request: TrustRequired) => void): Promise<UnlistenFn> {
	return listen<TrustRequired>(EVENTS.trustRequired, (event) => handler(event.payload));
}
