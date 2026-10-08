<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import type { Chip } from '$lib/printerState';

	let { chip }: { chip: Chip } = $props();
</script>

<span class="chip chip-{chip}">{t('chip', { chip })}</span>

<style>
	.chip-run {
		background: var(--accent-soft);
		color: var(--accent);
	}

	.chip-warn {
		background: var(--gold);
		color: var(--chip-ink);
	}

	.chip-fail,
	.chip-auth {
		background: var(--heat);
		color: var(--chip-ink);
	}

	.chip-pause {
		background: var(--warn-soft);
		color: var(--warn);
	}

	.chip-idle,
	.chip-done {
		background: var(--surface-2);
		color: var(--text);
	}

	.chip-wait {
		background: var(--surface-2);
		color: var(--text-muted);
	}

	.chip-off {
		border: 1px solid var(--border-strong);
		padding: 3px 5px;
		color: var(--text-muted);
	}
</style>
