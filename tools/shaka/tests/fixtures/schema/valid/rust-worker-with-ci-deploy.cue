package project

#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	worker: {}
	ci: {
		deploy: {
			reusableWorkflow:    "./.github/workflows/cf-deploy.yml"
			previewScriptPrefix: "kolohelios-portfolio"
		}
	}
}
