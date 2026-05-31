package project

#Project & {
	name: "claude-hooks"
	kind: "rust-cli"
	cli: {
		binaryName:       "claude-hooks"
		shellCompletions: true
		// Consumed by `infra/home/modules/common.nix` so the binary
		// is on PATH for every claude session, not just inside the
		// kolohelios checkout. Build deps stay minimal — no
		// rust-overlay nightly here, the source has no
		// nightly-only features and nixpkgs's stable rustc builds
		// it fine.
	}
	coverage: {
		line: {
			fail: 1
		}
	}
	ci: {
		build: {
			filterKey:   "claude-hooks"
			jobId:       "claude-hooks"
			displayName: "claude-hooks"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/claude-hooks"
				visibility: "public"
				rolling:    true
			}
		}
	}
}
