package project

// previewScriptPrefix must match the project-name regex
// (`^[a-z][a-z0-9-]*$`) — uppercase / leading digit should reject.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	ci: {
		deploy: {
			reusableWorkflow:    "./.github/workflows/cf-deploy.yml"
			previewScriptPrefix: "Portfolio"
		}
	}
}
