<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { t } from '$lib/i18nRuntime.svelte';
	import Icon from './Icon.svelte';
	import { check as checkForUpdate, type Update } from '@tauri-apps/plugin-updater';
	import { relaunch } from '@tauri-apps/plugin-process';

	const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

	let available = $state<{ version: string; body: string | null } | null>(null);
	let dismissedVersion = $state<string | null>(null);
	let installing = $state(false);
	let error = $state<string | null>(null);

	// The live `Update` handle from the plugin. Kept as a plain field, not
	// `$state`, since nothing in the template reads it directly and it carries
	// methods rather than data.
	let pendingUpdate: Update | null = null;

	async function check() {
		try {
			const update = await checkForUpdate();
			if (update) {
				pendingUpdate = update;
				available = { version: update.version, body: update.body ?? null };
			}
		} catch (err) {
			// The updater plugin is gated behind signing secrets that do not
			// exist yet (see CLAUDE.md's "known gaps"), so on most builds this
			// rejects because it is simply not configured. That is expected,
			// not a failure worth disrupting the rest of the UI over.
			console.warn('[update] check failed', err);
		}
	}

	async function install() {
		if (!pendingUpdate) return;
		installing = true;
		error = null;
		try {
			await pendingUpdate.downloadAndInstall();
			await relaunch();
		} catch (err) {
			error = t('update-install-error', { message: String(err) });
			installing = false;
		}
	}

	function dismiss() {
		if (available) dismissedVersion = available.version;
	}

	let interval: ReturnType<typeof setInterval> | undefined;

	onMount(() => {
		void check();
		interval = setInterval(() => void check(), CHECK_INTERVAL_MS);
	});

	onDestroy(() => {
		if (interval) clearInterval(interval);
	});

	const visible = $derived(available !== null && available.version !== dismissedVersion);
</script>

{#if visible && available}
	<div class="banner">
		<Icon name="spool" size={16} />
		<div class="text">
			<p class="title">{t('update-available', { version: available.version })}</p>
			{#if available.body}<p class="notes">{available.body}</p>{/if}
			{#if error}<p class="error">{error}</p>{/if}
		</div>
		<div class="actions">
			<button class="btn primary" disabled={installing} onclick={install}>
				{installing ? t('update-installing') : t('update-install')}
			</button>
			<button class="icon-btn" onclick={dismiss} aria-label={t('update-dismiss')}>
				<Icon name="close" size={14} />
			</button>
		</div>
	</div>
{/if}

<style>
	.banner {
		display: flex;
		align-items: flex-start;
		gap: 0.5rem;
		padding: 0.55rem 0.7rem;
		background: var(--accent-soft);
		border-bottom: 1px solid var(--border);
		flex-shrink: 0;
	}

	.banner :global(svg) {
		color: var(--accent);
		flex-shrink: 0;
		margin-top: 0.15rem;
	}

	.text {
		flex: 1 1 auto;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 0.1rem;
	}

	.title {
		font-size: 0.8rem;
		font-weight: 700;
	}

	.notes {
		font-size: 0.72rem;
		color: var(--text-muted);
		white-space: pre-wrap;
	}

	.actions {
		display: flex;
		align-items: center;
		gap: 0.2rem;
		flex-shrink: 0;
	}
</style>
