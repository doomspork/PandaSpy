/**
 * Conventional Commits, enforced on both pull-request titles and the commits
 * inside them.
 *
 * Both, because either can become the permanent history depending on how the
 * PR is merged: a squash merge keeps the title, a merge commit keeps the
 * commits. Release Please reads whichever one survives, so whichever one
 * survives has to be well-formed.
 *
 * @type {import('@commitlint/types').UserConfig}
 */
export default {
	extends: ['@commitlint/config-conventional'],
	rules: {
		// Scopes map to crates, plus the few cross-cutting areas that are not
		// crates. An unrecognised scope is almost always a typo or a change that
		// belongs somewhere else than the author thinks.
		'scope-enum': [
			2,
			'always',
			[
				// Crates, by their directory name minus the `bambu-` prefix.
				'proto', // crates/pandaspy-proto
				'discovery', // crates/pandaspy-discovery
				'client', // crates/pandaspy-client
				'store', // crates/pandaspy-store
				'tauri', // src-tauri
				'xtask', // xtask

				// Cross-cutting.
				'ui', // the SvelteKit frontend
				'i18n', // locales/ and the Fluent plumbing on both sides
				'fixtures', // the recorded payload corpus
				'ci', // workflows and the composite action
				'deps', // dependency bumps
				'release' // release-please, versioning, packaging
			]
		],
		// A scope is optional: `docs:` and repo-wide changes have none.
		'scope-empty': [0],
		// Bodies here explain *why*; that needs room, but not unbounded room.
		'body-max-line-length': [2, 'always', 100],
		'footer-max-line-length': [0]
	}
};
