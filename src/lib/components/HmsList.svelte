<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import type { HmsView } from '$lib/ipc';

	let { entries }: { entries: HmsView[] } = $props();
</script>

{#if entries.length > 0}
	<ul class="hms">
		{#each entries as entry, i (entry.code ?? i)}
			<li class="severity-{entry.severity ?? 'unknown'}">
				<span class="sev-label">{t('hms-severity', { severity: entry.severity ?? 'unknown' })}</span
				>
				<span class="text">
					{#if entry.text}
						{entry.text}
					{:else if entry.code}
						{t('hms-code-only', { code: entry.code })}
					{/if}
				</span>
				{#if entry.wikiUrl}
					<!-- `rel="external"` is also what tells eslint-plugin-svelte's
					     SvelteKit link-check that this is a genuine external URL,
					     not an internal route missing a `resolve()` call. -->
					<a href={entry.wikiUrl} target="_blank" rel="external noopener noreferrer"
						>{t('hms-learn-more')}</a
					>
				{/if}
			</li>
		{/each}
	</ul>
{/if}

<style>
	.hms {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
	}

	li {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: 0.4rem;
		padding: 0.35rem 0.5rem;
		border-radius: var(--radius-sm);
		border-left: 3px solid var(--border);
		background: var(--surface-2);
		font-size: 0.78rem;
	}

	.severity-fatal {
		border-left-color: var(--danger);
		background: var(--danger-soft);
	}

	.severity-serious {
		border-left-color: var(--danger);
	}

	.severity-common {
		border-left-color: var(--warn);
		background: var(--warn-soft);
	}

	.severity-info {
		border-left-color: var(--text-muted);
	}

	.sev-label {
		font-weight: 700;
		text-transform: uppercase;
		font-size: 0.65rem;
		letter-spacing: 0.03em;
		color: var(--text-muted);
	}

	.severity-fatal .sev-label,
	.severity-serious .sev-label {
		color: var(--danger);
	}

	.severity-common .sev-label {
		color: var(--warn);
	}

	.text {
		flex: 1 1 auto;
	}

	a {
		font-size: 0.72rem;
		white-space: nowrap;
	}
</style>
