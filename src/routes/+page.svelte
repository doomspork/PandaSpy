<script lang="ts">
	import { onMount } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';
	import {
		getSettings,
		listPrinters,
		onPrinterUpdate,
		onTrustRequired,
		removePrinter,
		reorderPrinters,
		setSettings,
		type PrinterView,
		type SettingsView,
		type TrustRequired
	} from '$lib/ipc';
	import { negotiate, setLocale, t } from '$lib/i18nRuntime.svelte';
	import PrinterList from '$lib/components/PrinterList.svelte';
	import AddPrinter from '$lib/components/AddPrinter.svelte';
	import Settings from '$lib/components/Settings.svelte';
	import TrustDialog from '$lib/components/TrustDialog.svelte';
	import UpdateBanner from '$lib/components/UpdateBanner.svelte';
	import Icon from '$lib/components/Icon.svelte';

	// Best guess before `getSettings()` resolves; overridden below the moment
	// a saved locale preference (or an explicit "follow system") comes back.
	setLocale(negotiate(navigator.languages ?? [navigator.language]));

	type View = 'list' | 'add' | 'settings';
	let view = $state<View>('list');

	let printers = $state<PrinterView[]>([]);
	let settings = $state<SettingsView | null>(null);
	let trustQueue = $state<TrustRequired[]>([]);
	let loadError = $state<string | null>(null);

	const activeTrust = $derived(trustQueue[0] ?? null);
	const activeTrustPrinter = $derived(
		activeTrust ? printers.find((p) => p.serial === activeTrust.serial) : undefined
	);

	// Serials removed locally but not necessarily settled on the Rust side yet.
	// A `printer://update` for one of these is stale — it was either already
	// in flight when `removePrinter` was called, or arrives just after — and
	// must not resurrect the card. Nothing reads this set directly in a
	// template, but `SvelteSet` (rather than a plain `Set`) is still correct
	// here since it's mutated from inside runes-mode component state.
	const removedSerials = new SvelteSet<string>();

	function upsertPrinter(printer: PrinterView) {
		if (removedSerials.has(printer.serial)) return;
		const index = printers.findIndex((p) => p.serial === printer.serial);
		printers =
			index === -1 ? [...printers, printer] : printers.map((p, i) => (i === index ? printer : p));
	}

	onMount(() => {
		let unlistenUpdate: (() => void) | undefined;
		let unlistenTrust: (() => void) | undefined;
		let cancelled = false;

		(async () => {
			try {
				printers = await listPrinters();
			} catch (err) {
				loadError = String(err);
			}

			try {
				const loaded = await getSettings();
				settings = loaded;
				if (loaded.locale) setLocale(loaded.locale);
			} catch {
				// The negotiated default above stays in effect. Settings itself
				// needs a working getSettings() to even open, so there is
				// nothing more useful to do with this failure here.
			}

			const [update, trust] = await Promise.all([
				onPrinterUpdate(upsertPrinter),
				onTrustRequired((request) => {
					trustQueue = [...trustQueue, request];
				})
			]);
			if (cancelled) {
				update();
				trust();
				return;
			}
			unlistenUpdate = update;
			unlistenTrust = trust;
		})();

		return () => {
			cancelled = true;
			unlistenUpdate?.();
			unlistenTrust?.();
		};
	});

	async function handleReorder(serials: string[]) {
		const bySerial = new Map(printers.map((p) => [p.serial, p]));
		const previous = printers;
		printers = serials
			.map((serial) => bySerial.get(serial))
			.filter((p): p is PrinterView => p !== undefined);
		try {
			await reorderPrinters(serials);
		} catch (err) {
			printers = previous;
			loadError = String(err);
		}
	}

	async function handleRemove(serial: string) {
		const previous = printers;
		removedSerials.add(serial);
		printers = printers.filter((p) => p.serial !== serial);
		try {
			await removePrinter(serial);
			// Defensive: drop anything that landed for this serial in the gap
			// between the optimistic filter above and `removedSerials` taking
			// effect for `upsertPrinter`.
			printers = printers.filter((p) => p.serial !== serial);
		} catch (err) {
			// The removal did not happen, so this serial's updates are live
			// again — undo the guard along with the optimistic removal.
			removedSerials.delete(serial);
			printers = previous;
			loadError = String(err);
		}
	}

	async function handleSettingsSave(next: { locale: string | null; launchAtLogin: boolean }) {
		await setSettings(next);
		settings = settings ? { ...settings, ...next } : settings;
		setLocale(next.locale ?? negotiate(navigator.languages ?? [navigator.language]));
	}
</script>

<svelte:head>
	<title>{t('window-title')}</title>
</svelte:head>

<div class="app">
	<UpdateBanner />

	{#if view === 'list'}
		<div class="screen">
			<header class="bar">
				<h1>{t('window-title')}</h1>
				<div class="bar-actions">
					<button class="icon-btn" onclick={() => (view = 'add')} aria-label={t('nav-add-printer')}>
						<Icon name="plus" size={16} />
					</button>
					<button
						class="icon-btn"
						disabled={!settings}
						onclick={() => settings && (view = 'settings')}
						aria-label={t('nav-settings')}
					>
						<Icon name="gear" size={16} />
					</button>
				</div>
			</header>
			<div class="scroll">
				{#if loadError}<p class="error load-error">{loadError}</p>{/if}
				<PrinterList
					{printers}
					onReorder={handleReorder}
					onRemove={handleRemove}
					onAddPrinter={() => (view = 'add')}
				/>
			</div>
		</div>
	{:else if view === 'add'}
		<AddPrinter
			{settings}
			onBack={() => (view = 'list')}
			onDone={(serial) => {
				removedSerials.delete(serial);
				view = 'list';
			}}
		/>
	{:else if view === 'settings' && settings}
		<Settings {settings} onBack={() => (view = 'list')} onSave={handleSettingsSave} />
	{/if}
</div>

{#if activeTrust}
	<TrustDialog
		request={activeTrust}
		printer={activeTrustPrinter}
		onResolved={() => (trustQueue = trustQueue.slice(1))}
	/>
{/if}

<style>
	.app {
		display: flex;
		flex-direction: column;
		height: 100vh;
	}

	.screen {
		display: flex;
		flex-direction: column;
		flex: 1 1 auto;
		min-height: 0;
	}

	.bar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.6rem 0.75rem;
		border-bottom: 1px solid var(--border);
		flex-shrink: 0;
	}

	.bar h1 {
		font-size: 1rem;
		font-weight: 700;
		letter-spacing: -0.01em;
	}

	.bar-actions {
		display: flex;
		gap: 0.1rem;
	}

	.scroll {
		flex: 1 1 auto;
		overflow-y: auto;
		min-height: 0;
	}

	.load-error {
		padding: 0.6rem 0.75rem 0;
	}
</style>
