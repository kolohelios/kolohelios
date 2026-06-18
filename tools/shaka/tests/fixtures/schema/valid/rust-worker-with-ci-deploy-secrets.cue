package project

// A rust-worker with a hand-authored wrangler.toml declares the Worker
// runtime secrets to push at deploy via `ci.deploy.secrets` (names only),
// without a `wrangler:` block that would enroll it in generate-wrangler
// drift. `push-worker-secrets` reads these alongside `wrangler.secrets`.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	worker: {}
	ci: {
		deploy: {
			reusableWorkflow:    "./.github/workflows/cf-deploy.yml"
			previewScriptPrefix: "kolohelios-portfolio"
			secrets: ["OPENROUTER_API_KEY", "KIT_API_KEY"]
		}
	}
}
