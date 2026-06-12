package project

#Project & {
	name: "shaka"
	kind: "rust-cli"
	cli: {
		binaryName:       "shaka"
		description:      "shaka — build tooling for kolohelios"
		shellCompletions: true
		// `cue`/`jujutsu`/`git`/`opentofu` are needed by integration
		// tests (`tests/schema.rs`, `tests/workspace.rs`,
		// `tests/terraform_emit.rs`) that shell out during the build
		// sandbox's check phase. Without them, `nix flake check`
		// (and `shaka preflight`) fails.
		checkInputs: [
			"cue",
			"jujutsu",
			"git",
			"opentofu",
		]
		// Bake shaka's everyday runtime deps onto the wrapped
		// binary's PATH so `nix run ./tools/shaka -- <subcmd>` works
		// without a dev shell — that's how non-preflight CI
		// workflows (auto-rebase, kolohelios-nix bump) skip the heavy
		// Rust toolchain fetch. Less-common shaka subcommands
		// (object-store, preflight) still expect the dev shell.
		runtimePathDeps: [
			"jujutsu",
			"git",
			"gh",
			"nix",
			"cue",
			"just",
			"jq",
		]
		extraDevShellPackages: [
			// actionlint runs in `shaka preflight` against
			// `.github/workflows/`; shellcheck is its dependency
			// for linting embedded `run:` scripts.
			"actionlint",
			"shellcheck",
			// opentofu is needed by `shaka terraform emit` and the
			// integration tests under `tests/terraform_emit.rs`.
			"opentofu",
			// taplo runs the `taplo fmt --check` preflight gate.
			"taplo",
			// `shaka domain check` shells out to `whois` and `dig`.
			"whois",
			"dnsutils",
		]
		// Bundle the project schema + a `cue.mod` + a permissive stub
		// registry so schema-dependent subcommands work from a nix-built
		// `shaka` run outside the monorepo (#741). `cue-bundle/` carries
		// the `cue.mod` and stub; `schema/` ships at its module-relative
		// path so the schema's absolute imports resolve.
		cueModule: {
			include: [
				{from: "cue-bundle", to: "."},
				{from: "schema", to: "tools/shaka/schema"},
			]
		}
	}
	coverage: {
		line: {
			fail: 62
		}
	}
	ci: {
		build: {
			filterKey:   "shaka"
			jobId:       "shaka"
			displayName: "shaka"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/shaka"
				visibility: "public"
				rolling:    true
			}
		}
	}
}
