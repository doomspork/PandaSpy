<script lang="ts">
	import { untrack } from 'svelte';
	import { availableLocales, t } from '$lib/i18nRuntime.svelte';
	import { displayName } from '$lib/printerState';
	import Icon from './Icon.svelte';
	import type { PrinterView, SettingsView } from '$lib/ipc';

	let {
		settings,
		printers,
		onBack,
		onSave,
		onReorder,
		onRemove
	}: {
		settings: SettingsView;
		printers: PrinterView[];
		onBack: () => void;
		onSave: (next: { locale: string | null; launchAtLogin: boolean }) => Promise<void>;
		onReorder: (serials: string[]) => void;
		onRemove: (serial: string) => void;
	} = $props();

	// Removing a printer and ordering the list live here rather than on the
	// popover: a trash can one mis-click away from a glance at a print is the
	// wrong place for something that forgets an access code. The order set
	// here breaks ties in the popover, which otherwise puts printers that
	// need attention first.
	let confirmingRemove = $state<string | null>(null);

	function move(index: number, by: -1 | 1) {
		const order = printers.map((p) => p.serial);
		const target = index + by;
		if (target < 0 || target >= order.length) return;
		[order[index], order[target]] = [order[target], order[index]];
		onReorder(order);
	}

	// `settings` only seeds the initial form values; once the user is editing,
	// this screen owns them until `persist()` writes back through `onSave`.
	// `untrack` says that deliberately, rather than leaving a warning that a
	// change to `settings` elsewhere should have been mirrored in here.
	let locale = $state(untrack(() => settings.locale));
	let launchAtLogin = $state(untrack(() => settings.launchAtLogin));
	let error = $state<string | null>(null);

	// Reverts to whatever was on screen before the change being persisted, so
	// a rejected `onSave` doesn't leave a control showing a value that was
	// never actually saved.
	async function persist(previous: { locale: string | null; launchAtLogin: boolean }) {
		error = null;
		try {
			await onSave({ locale, launchAtLogin });
		} catch (err) {
			locale = previous.locale;
			launchAtLogin = previous.launchAtLogin;
			error = t('settings-save-error', { message: String(err) });
		}
	}

	function handleLocaleChange(value: string) {
		const previous = { locale, launchAtLogin };
		locale = value === '' ? null : value;
		void persist(previous);
	}

	function handleLaunchToggle() {
		const previous = { locale, launchAtLogin };
		launchAtLogin = !launchAtLogin;
		void persist(previous);
	}
</script>

<div class="screen">
	<header class="bar">
		<button class="icon-btn" onclick={onBack} aria-label={t('nav-back')}>
			<Icon name="chevron-left" size={16} />
		</button>
		<h2>{t('settings-title')}</h2>
	</header>

	<div class="body">
		{#if printers.length > 0}
			<section class="printers">
				<h3 class="label">{t('settings-printers')}</h3>
				<ul>
					{#each printers as printer, i (printer.serial)}
						{@const name = displayName(printer)}
						<li>
							{#if confirmingRemove === printer.serial}
								<div class="confirm">
									<p>{t('card-remove-confirm-title', { name })}</p>
									<p class="muted">{t('card-remove-confirm-body')}</p>
									<div class="confirm-actions">
										<button class="btn" onclick={() => (confirmingRemove = null)}
											>{t('common-cancel')}</button
										>
										<button
											class="btn danger"
											onclick={() => {
												confirmingRemove = null;
												onRemove(printer.serial);
											}}>{t('card-remove-confirm-confirm')}</button
										>
									</div>
								</div>
							{:else}
								<span class="printer-name">{name}</span>
								<button
									class="icon-btn"
									disabled={i === 0}
									onclick={() => move(i, -1)}
									aria-label={t('settings-move-up', { name })}
								>
									<Icon name="chevron-up" size={14} />
								</button>
								<button
									class="icon-btn"
									disabled={i === printers.length - 1}
									onclick={() => move(i, 1)}
									aria-label={t('settings-move-down', { name })}
								>
									<Icon name="chevron-down" size={14} />
								</button>
								<button
									class="icon-btn danger"
									onclick={() => (confirmingRemove = printer.serial)}
									aria-label={t('card-remove-named', { name })}
								>
									<Icon name="trash" size={14} />
								</button>
							{/if}
						</li>
					{/each}
				</ul>
			</section>
		{/if}

		<label class="field">
			<span>{t('settings-language')}</span>
			<select
				value={locale ?? ''}
				onchange={(event) => handleLocaleChange(event.currentTarget.value)}
			>
				<option value="">{t('settings-language-system')}</option>
				{#each availableLocales as tag (tag)}
					<option value={tag}>{tag}</option>
				{/each}
			</select>
		</label>

		<label class="field row">
			<span>{t('settings-launch-at-login')}</span>
			<input type="checkbox" checked={launchAtLogin} onchange={handleLaunchToggle} />
		</label>

		<p class="hint">
			{t('settings-secrets', { backend: settings.secretBackend, keyring: settings.keyringName })}
		</p>

		{#if error}<p class="error">{error}</p>{/if}
	</div>
</div>

<style>
	.screen {
		display: flex;
		flex-direction: column;
		height: 100%;
	}

	.bar {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.6rem 0.75rem;
		border-bottom: 1px solid var(--border);
	}

	.bar h2 {
		font-size: 0.95rem;
	}

	.body {
		padding: 0.9rem 0.75rem;
		display: flex;
		flex-direction: column;
		gap: 0.9rem;
		overflow-y: auto;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
		font-size: 0.82rem;
		font-weight: 600;
	}

	.field.row {
		flex-direction: row;
		align-items: center;
		justify-content: space-between;
	}

	select {
		border: 1px solid var(--border);
		background: var(--surface);
		border-radius: var(--radius-sm);
		padding: 0.4rem 0.55rem;
		font-size: 0.82rem;
		font-weight: 400;
	}

	.printers {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.printers h3 {
		font-weight: 400;
	}

	.printers ul {
		list-style: none;
		margin: 0;
		padding: 0;
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		background: var(--surface);
	}

	.printers li {
		display: flex;
		align-items: center;
		gap: 0.1rem;
		padding: 0.15rem 0.3rem 0.15rem 0.7rem;
	}

	.printers li + li {
		border-top: 1px solid var(--border);
	}

	.printer-name {
		flex: 1 1 auto;
		min-width: 0;
		font-size: 0.85rem;
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.icon-btn:disabled {
		opacity: 0.35;
		cursor: default;
	}

	.confirm {
		flex: 1 1 auto;
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
		padding: 0.5rem 0.2rem;
		font-size: 0.8rem;
	}

	.confirm-actions {
		display: flex;
		gap: 0.4rem;
		justify-content: flex-end;
		margin-top: 0.2rem;
	}

	.hint {
		font-size: 0.75rem;
		color: var(--text-muted);
		border-top: 1px solid var(--border);
		padding-top: 0.7rem;
	}
</style>
