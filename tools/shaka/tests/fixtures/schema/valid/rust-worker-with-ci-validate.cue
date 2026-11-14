package project

#Project & {
	name: "worker"
	kind: "rust-worker"
	worker: {}
	ci: {
		validate: {}
		deploy: {
			reusableWorkflow: "kolohelios/kolohelios/.github/workflows/cf-deploy.yml@main"
			previewScriptPrefix: "worker"
		}
	}
}
