<script lang="ts">
	import { untrack } from 'svelte';
	import { availableLocales, t } from '$lib/i18nRuntime.svelte';
	import Icon from './Icon.svelte';
	import type { SettingsView } from '$lib/ipc';

	let {
		settings,
		onBack,
		onSave
	}: {
		settings: SettingsView;
		onBack: () => void;
		onSave: (next: { locale: string | null; launchAtLogin: boolean }) => Promise<void>;
	} = $props();

	// `settings` only seeds the initial form values; once the user is editing,
	// this screen owns them until `persist()` writes back through `onSave`.
	// `untrack` says that deliberately, rather than leaving a warning that a
	// change to `settings` elsewhere should have been mirrored in here.
	let locale = $state(untrack(() => settings.locale));
	let launchAtLogin = $state(untrack(() => settings.launchAtLogin));
	let error = $state<string | null>(null);

	async function persist() {
		error = null;
		try {
			await onSave({ locale, launchAtLogin });
		} catch (err) {
			error = t('settings-save-error', { message: String(err) });
		}
	}

	function handleLocaleChange(value: string) {
		locale = value === '' ? null : value;
		void persist();
	}

	function handleLaunchToggle() {
		launchAtLogin = !launchAtLogin;
		void persist();
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

	.hint {
		font-size: 0.75rem;
		color: var(--text-muted);
		border-top: 1px solid var(--border);
		padding-top: 0.7rem;
	}
</style>
