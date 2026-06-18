package project

// `ci.deploy.secrets` entries must be SCREAMING_SNAKE_CASE
// (`^[A-Z][A-Z0-9_]*$`) to match a real env var / `wrangler secret`
// name; a lowercase name is rejected.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	worker: {}
	ci: {
		deploy: {
			reusableWorkflow:    "./.github/workflows/cf-deploy.yml"
			previewScriptPrefix: "kolohelios-portfolio"
			secrets: ["openrouter_api_key"]
		}
	}
}
