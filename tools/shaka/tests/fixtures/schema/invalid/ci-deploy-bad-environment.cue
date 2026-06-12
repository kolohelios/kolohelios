package project

// `environment` must be a wrangler-style lowercase identifier; an
// uppercase value is rejected.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	ci: {
		deploy: {
			reusableWorkflow:    "./.github/workflows/cf-deploy.yml"
			previewScriptPrefix: "buzzingo"
			environment:         "Production"
		}
	}
}
