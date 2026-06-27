package project

#Project & {
	name: "scoretracker"
	kind: "rust-worker"
	worker: {
		// `cue` exports the static game configs in `games/*.cue` to JSON at
		// build time (see `build.rs`); the result is embedded with
		// `include_str!`. cf-deploy runs `worker-build` inside this
		// devshell, so `cue` is on PATH in CI too.
		extraDevShellPackages: ["cue"]
	}

	// No `wrangler:` block: this Worker has a Durable Object
	// (`GameState`) and the migration that registers it, neither of which
	// `#Wrangler` models. Its `wrangler.toml` is hand-authored (same as
	// `services/notes-sync`); `shaka deploy generate-wrangler` only
	// generates/drift-checks projects that declare a `wrangler:` block.

	serving: [
		{
			via: "cloudflare-worker"
			hostnames: ["scoretracker.kolohelios.com"]
		},
	]
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "scoretracker.kolohelios.com"
		zone:         "kolohelios.com"
	}
	ci: {
		deploy: {
			reusableWorkflow:    "./.github/workflows/cf-deploy.yml"
			previewScriptPrefix: "scoretracker"
			// No runtime secrets: the app is fully public, no login.
		}
	}
}
