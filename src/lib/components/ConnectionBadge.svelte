<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import type { ConnectionView } from '$lib/ipc';

	let { connection }: { connection: ConnectionView } = $props();
</script>

<span class="badge status-{connection.status}">
	<span class="dot"></span>
	{t('connection-status', { status: connection.status })}
</span>
{#if connection.status === 'failed' && connection.reason}
	<p class="reason">{t('connection-reason', { reason: connection.reason })}</p>
{/if}

<style>
	.badge {
		display: inline-flex;
		align-items: center;
		gap: 0.35em;
		font-size: 0.75rem;
		font-weight: 600;
		color: var(--text-muted);
	}

	.dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--text-muted);
	}

	.status-connected .dot {
		background: var(--accent);
	}

	.status-connected {
		color: var(--accent);
	}

	.status-connecting .dot,
	.status-handshaking .dot {
		background: var(--warn);
		animation: pulse 1.2s ease-in-out infinite;
	}

	.status-failed .dot {
		background: var(--danger);
	}

	.status-failed {
		color: var(--danger);
	}

	.reason {
		font-size: 0.75rem;
		color: var(--danger);
		margin-top: 2px;
	}

	@keyframes pulse {
		0%,
		100% {
			opacity: 1;
		}
		50% {
			opacity: 0.35;
		}
	}
</style>
