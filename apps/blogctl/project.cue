package project

#Project & {
	name: "blogctl"
	kind: "rust-cli"
	cli: {
		binaryName:       "blogctl"
		shellCompletions: true
		// Integration tests in `tests/cli.rs` spawn `jj` directly
		// (and exercise blogctl commands whose precondition checks
		// shell out to `jj status`). Without `jujutsu` on PATH the
		// test suite fails inside the nix-build sandbox.
		checkInputs: ["jujutsu"]
	}
	coverage: {
		// Floor picked from the current measured line coverage minus
		// ~7% headroom: as of #442, line is ~92%. Bump as targeted
		// tests fill remaining gaps.
		line: {
			fail: 85
		}
	}
	ci: {
		build: {
			filterKey:   "blogctl"
			jobId:       "blogctl"
			displayName: "blogctl"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/blogctl"
				visibility: "public"
				rolling:    true
				// Consumers (e.g. kolohelios/blogs-and-posts) build
				// blogctl from this FlakeHub URL; without the second
				// build step, our local-path build's closure misses
				// their cache lookups. See #591.
				populateConsumerCache: true
			}
			// Notify the blogs-and-posts repo of a fresh publish so its
			// bump-blogctl workflow runs immediately rather than waiting
			// for its daily cron.
			dispatch: [
				{
					repo:      "kolohelios/blogs-and-posts"
					eventType: "blogctl-published"
				},
			]
		}
	}
}
