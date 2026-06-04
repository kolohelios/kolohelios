package project

#Project & {
	name: "aof"
	kind: "rust-cli"
	cli: {
		binaryName:       "aof"
		package:          true
		shellCompletions: true
		// `aof render` shells out to `cue export` (load the areas tree)
		// and `d2` (emit the diagram SVG). Wrap both onto the packaged
		// binary's PATH so it's self-contained off FlakeHub; `cue` also
		// rides along in the devShell via `workflowPackages`, but `d2`
		// is aof-specific.
		runtimePathDeps: ["cue", "d2"]
		extraDevShellPackages: ["d2"]
	}
	coverage: {
		line: {
			fail: 1
		}
	}
	ci: {
		build: {
			filterKey:   "aof"
			jobId:       "aof"
			displayName: "aof"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/aof"
				visibility: "public"
				rolling:    true
				// kolohelios/personal-os (#620) consumes aof from
				// FlakeHub; without the second build step, the
				// consumer-side .drv lookup misses our cache. See #591
				// for the pattern this preempts.
				populateConsumerCache: true
			}
			// Notify personal-os of a fresh publish so its bump
			// workflow runs immediately rather than waiting for cron.
			// The repo doesn't exist yet (#620); until it does, this
			// dispatch step 404s while the publish itself succeeds.
			dispatch: [
				{
					repo:      "kolohelios/personal-os"
					eventType: "aof-published"
				},
			]
		}
	}
}
