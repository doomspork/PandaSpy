<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import Icon from './Icon.svelte';
	import { resolveTrust, type PrinterView, type TrustRequired } from '$lib/ipc';

	let {
		request,
		printer,
		onResolved
	}: {
		request: TrustRequired;
		printer: PrinterView | undefined;
		onResolved: () => void;
	} = $props();

	let busy = $state(false);
	let error = $state<string | null>(null);

	// This dialog owns the `resolveTrust` call itself (rather than just
	// signalling a boolean up) so a rejected decision — network hiccup, stale
	// session — leaves the dialog open with the error shown instead of
	// silently dropping the prompt and reconnecting to an unverified printer.
	async function decide(accept: boolean) {
		busy = true;
		error = null;
		try {
			await resolveTrust(request.serial, accept);
			onResolved();
		} catch (err) {
			error = t('trust-error', { message: String(err) });
		} finally {
			busy = false;
		}
	}
</script>

<div class="backdrop">
	<div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="trust-dialog-title">
		<div class="dialog-head">
			<Icon name="warning" size={20} />
			<h2 id="trust-dialog-title">{t('trust-title')}</h2>
		</div>

		<p class="printer-name">{printer?.nickname || printer?.model || request.serial}</p>
		<p class="body">{t('trust-body')}</p>

		<dl class="fingerprints">
			<div>
				<dt>{t('trust-pinned-label')}</dt>
				<dd><code>{request.pinned}</code></dd>
			</div>
			<div>
				<dt>{t('trust-presented-label')}</dt>
				<dd><code>{request.presented}</code></dd>
			</div>
		</dl>

		{#if error}<p class="error">{error}</p>{/if}

		<div class="actions">
			<button class="btn" disabled={busy} onclick={() => decide(false)}>{t('trust-reject')}</button>
			<button class="btn danger" disabled={busy} onclick={() => decide(true)}
				>{t('trust-accept')}</button
			>
		</div>
	</div>
</div>

<style>
	.backdrop {
		position: fixed;
		inset: 0;
		background: rgba(10, 8, 6, 0.55);
		backdrop-filter: blur(1px);
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 1rem;
		z-index: 100;
	}

	.dialog {
		width: 100%;
		max-width: 340px;
		background: var(--surface);
		border: 1px solid var(--danger);
		border-radius: var(--radius-md);
		padding: 1rem;
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
		box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
	}

	.dialog-head {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		color: var(--danger);
	}

	.dialog-head h2 {
		font-size: 0.95rem;
	}

	.printer-name {
		font-weight: 700;
	}

	.body {
		font-size: 0.8rem;
		color: var(--text-muted);
	}

	.fingerprints {
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
		background: var(--surface-2);
		border-radius: var(--radius-sm);
		padding: 0.5rem 0.6rem;
	}

	.fingerprints dt {
		font-size: 0.68rem;
		text-transform: uppercase;
		letter-spacing: 0.03em;
		color: var(--text-muted);
	}

	.fingerprints dd {
		margin: 0;
		font-family: var(--font-mono);
		font-size: 0.72rem;
		word-break: break-all;
	}

	.actions {
		display: flex;
		gap: 0.5rem;
		justify-content: flex-end;
		margin-top: 0.2rem;
	}
</style>
