<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import type { HmsView } from '$lib/ipc';

	let { entries }: { entries: HmsView[] } = $props();
</script>

{#if entries.length > 0}
	<ul class="hms">
		{#each entries as entry, i (entry.code ?? i)}
			<li>
				<span class="chip severity-{entry.severity ?? 'unknown'}"
					>{t('hms-severity', { severity: entry.severity ?? 'unknown' })}</span
				>
				<p class="text">
					{#if entry.text}
						{entry.text}
					{:else if entry.code}
						{t('hms-code-only', { code: entry.code })}
					{/if}
					{#if entry.wikiUrl}
						<!-- `rel="external"` is also what tells eslint-plugin-svelte's
						     SvelteKit link-check that this is a genuine external URL,
						     not an internal route missing a `resolve()` call. -->
						<a href={entry.wikiUrl} target="_blank" rel="external noopener noreferrer"
							>{t('hms-learn-more')}</a
						>
					{/if}
				</p>
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
	}

	li {
		display: flex;
		align-items: flex-start;
		gap: 10px;
		padding: 10px 0 0;
		border-top: 1px dashed var(--border-strong);
	}

	li + li {
		margin-top: 10px;
	}

	.chip {
		margin-top: 1px;
	}

	.severity-fatal,
	.severity-serious {
		background: var(--heat);
		color: var(--chip-ink);
	}

	.severity-common {
		background: var(--gold);
		color: var(--chip-ink);
	}

	.severity-info,
	.severity-unknown {
		background: var(--surface-2);
		color: var(--text-muted);
	}

	.text {
		flex: 1 1 auto;
		min-width: 0;
		font-size: 12.5px;
		line-height: 1.4;
	}

	a {
		margin-left: 0.3em;
		font-family: var(--font-mono);
		font-size: 10px;
		letter-spacing: 0.06em;
		text-transform: uppercase;
		white-space: nowrap;
		color: var(--warn);
	}
</style>
