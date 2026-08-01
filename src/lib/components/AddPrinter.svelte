<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import Icon from './Icon.svelte';
	import {
		addPrinter,
		discoverPrinters,
		importStudio,
		type DiscoveredView,
		type DiscoveryReport,
		type SettingsView,
		type StudioPrinterView
	} from '$lib/ipc';

	let {
		settings,
		onBack,
		onDone
	}: {
		settings: SettingsView | null;
		onBack: () => void;
		onDone: () => void;
	} = $props();

	type Tab = 'discovered' | 'manual' | 'studio';
	let tab = $state<Tab>('discovered');

	// Discovered tab.
	let scanning = $state(false);
	let discovery = $state<DiscoveryReport | null>(null);
	let discoverError = $state<string | null>(null);

	// Bambu Studio import tab.
	let importing = $state(false);
	let studioResults = $state<StudioPrinterView[] | null>(null);
	let studioError = $state<string | null>(null);

	// Manual form — also where "use this printer" from the other two tabs lands,
	// so the user only has to type the access code.
	let serial = $state('');
	let address = $state('');
	let accessCode = $state('');
	let nickname = $state('');
	let submitting = $state(false);
	let formError = $state<string | null>(null);

	async function scan() {
		scanning = true;
		discoverError = null;
		try {
			discovery = await discoverPrinters();
		} catch (err) {
			discoverError = String(err);
		} finally {
			scanning = false;
		}
	}

	async function runImport() {
		importing = true;
		studioError = null;
		try {
			studioResults = await importStudio();
		} catch (err) {
			studioError = String(err);
		} finally {
			importing = false;
		}
	}

	function useDiscovered(found: DiscoveredView) {
		serial = found.serial ?? '';
		address = found.address;
		nickname = found.name ?? '';
		tab = 'manual';
	}

	function useStudio(printer: StudioPrinterView) {
		serial = printer.serial ?? '';
		address = printer.address ?? '';
		nickname = printer.name ?? '';
		tab = 'manual';
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		formError = null;
		if (!serial.trim() || !accessCode.trim()) {
			formError = t('add-printer-manual-error-required');
			return;
		}
		submitting = true;
		try {
			await addPrinter({
				serial: serial.trim(),
				address: address.trim(),
				accessCode,
				nickname: nickname.trim() || null
			});
			onDone();
		} catch (err) {
			formError = t('add-printer-manual-error', { message: String(err) });
		} finally {
			submitting = false;
		}
	}

	// Scan as soon as the Discovered tab is first shown, rather than making
	// the user press a button to see anything.
	$effect(() => {
		if (tab === 'discovered' && discovery === null && !scanning) {
			scan();
		}
	});
</script>

<div class="screen">
	<header class="bar">
		<button class="icon-btn" onclick={onBack} aria-label={t('nav-back')}>
			<Icon name="chevron-left" size={16} />
		</button>
		<h2>{t('add-printer-title')}</h2>
	</header>

	<nav class="tabs">
		<button class:active={tab === 'discovered'} onclick={() => (tab = 'discovered')}>
			{t('add-printer-tab-discovered')}
		</button>
		<button class:active={tab === 'manual'} onclick={() => (tab = 'manual')}>
			{t('add-printer-tab-manual')}
		</button>
		<button class:active={tab === 'studio'} onclick={() => (tab = 'studio')}>
			{t('add-printer-tab-studio')}
		</button>
	</nav>

	<div class="tab-body">
		{#if tab === 'discovered'}
			<div class="toolbar">
				<button class="btn" onclick={scan} disabled={scanning}>
					<Icon name="refresh" size={14} />
					{scanning ? t('add-printer-scanning') : t('add-printer-rescan')}
				</button>
			</div>
			{#if discoverError}
				<p class="error">{discoverError}</p>
			{:else if scanning && !discovery}
				<p class="muted">{t('add-printer-scanning')}</p>
			{:else if discovery}
				{#if discovery.printers.length > 0}
					<p class="muted">{t('discover-found-count', { count: discovery.printers.length })}</p>
					<ul class="found-list">
						{#each discovery.printers as found (found.address + (found.serial ?? ''))}
							<li>
								<div class="found-main">
									<strong>{found.name ?? found.model ?? found.address}</strong>
									<span class="muted">{found.address}</span>
								</div>
								{#if found.alreadyAdded}
									<span class="muted">{t('add-printer-already-added')}</span>
								{:else}
									<button class="btn" onclick={() => useDiscovered(found)}>
										{t('add-printer-use')}
									</button>
								{/if}
							</li>
						{/each}
					</ul>
				{:else}
					<p class="muted">{t('discover-verdict-empty', { verdict: discovery.verdict })}</p>
				{/if}
			{/if}
		{:else if tab === 'manual'}
			<form onsubmit={submit}>
				<label>
					{t('add-printer-manual-serial')}
					<input type="text" bind:value={serial} autocomplete="off" />
				</label>
				<label>
					{t('add-printer-manual-address')}
					<input type="text" bind:value={address} autocomplete="off" />
				</label>
				<label>
					{t('add-printer-manual-access-code')}
					<input type="password" bind:value={accessCode} autocomplete="off" />
				</label>
				<label>
					{t('add-printer-manual-nickname')}
					<input type="text" bind:value={nickname} autocomplete="off" />
				</label>
				<p class="hint">
					{t('add-printer-manual-access-code-hint', { keyring: settings?.keyringName ?? '' })}
				</p>
				{#if formError}<p class="error">{formError}</p>{/if}
				<button class="btn primary" type="submit" disabled={submitting}>
					{t('add-printer-manual-submit')}
				</button>
			</form>
		{:else if tab === 'studio'}
			<div class="toolbar">
				<button class="btn" onclick={runImport} disabled={importing}>
					<Icon name="refresh" size={14} />
					{t('add-printer-studio-import')}
				</button>
			</div>
			{#if studioError}
				<p class="error">{studioError}</p>
			{:else if importing}
				<p class="muted">{t('add-printer-studio-importing')}</p>
			{:else if studioResults}
				{#if studioResults.length > 0}
					<ul class="found-list">
						{#each studioResults as sp, i (sp.serial ?? i)}
							<li>
								<div class="found-main">
									<strong>{sp.name ?? sp.serial ?? sp.address}</strong>
									{#if sp.address}<span class="muted">{sp.address}</span>{/if}
								</div>
								<button class="btn" onclick={() => useStudio(sp)}
									>{t('add-printer-studio-use')}</button
								>
							</li>
						{/each}
					</ul>
				{:else}
					<p class="muted">{t('add-printer-studio-empty')}</p>
				{/if}
			{/if}
		{/if}
	</div>
</div>

<style>
	.screen {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}

	.bar {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.6rem 0.75rem;
		border-bottom: 1px solid var(--border);
		flex-shrink: 0;
	}

	.bar h2 {
		font-size: 0.95rem;
	}

	.tabs {
		display: flex;
		border-bottom: 1px solid var(--border);
		flex-shrink: 0;
	}

	.tabs button {
		flex: 1 1 0;
		background: none;
		border: none;
		padding: 0.55rem 0.4rem;
		font-size: 0.78rem;
		font-weight: 600;
		color: var(--text-muted);
		cursor: pointer;
		border-bottom: 2px solid transparent;
	}

	.tabs button.active {
		color: var(--accent);
		border-bottom-color: var(--accent);
	}

	.tab-body {
		flex: 1 1 auto;
		overflow-y: auto;
		padding: 0.75rem;
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
	}

	.toolbar {
		display: flex;
		justify-content: flex-end;
	}

	.found-list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.found-list li {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 0.5rem;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 0.5rem 0.6rem;
	}

	.found-main {
		display: flex;
		flex-direction: column;
		gap: 0.1rem;
		min-width: 0;
	}

	.found-main strong {
		font-size: 0.82rem;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.found-main .muted {
		font-family: var(--font-mono);
		font-size: 0.72rem;
	}

	form {
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
	}

	label {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		font-size: 0.78rem;
		font-weight: 600;
	}

	input {
		border: 1px solid var(--border);
		background: var(--surface);
		border-radius: var(--radius-sm);
		padding: 0.4rem 0.55rem;
		font-size: 0.82rem;
		font-weight: 400;
	}

	input:focus {
		outline: 2px solid var(--accent-soft);
		border-color: var(--accent);
	}

	.hint {
		font-size: 0.72rem;
		color: var(--text-muted);
	}
</style>
