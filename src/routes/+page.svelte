<script lang="ts">
	import { negotiate, setLocale, t } from '$lib/i18n';

	// Placeholder window. The real UI is a later session's job — this exists to
	// prove the shell launches and that the frontend resolves strings from the
	// same locales/ directory the Rust side reads.
	setLocale(negotiate(navigator.languages ?? [navigator.language]));

	const title = t('window-title');
</script>

<svelte:head>
	<title>{title}</title>
</svelte:head>

<main>
	<h1>{title}</h1>
</main>

<style>
	main {
		display: grid;
		place-items: center;
		min-height: 100vh;
		margin: 0;
		font-family:
			system-ui,
			-apple-system,
			'Segoe UI',
			sans-serif;
	}

	h1 {
		font-size: 2rem;
		font-weight: 600;
		letter-spacing: -0.02em;
	}

	/* The tray window sits over whatever the user is doing; match their theme
	   rather than flashing white over a dark desktop at 2am. */
	:global(body) {
		margin: 0;
		background: #ffffff;
		color: #1a1a1a;
	}

	@media (prefers-color-scheme: dark) {
		:global(body) {
			background: #1a1a1a;
			color: #f2f2f2;
		}
	}
</style>
