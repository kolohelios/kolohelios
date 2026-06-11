package project

// An external-repo consumer (e.g. the private buzzingo repo) reuses
// this repo's `cf-deploy.yml` via a cross-repo `uses:` reference rather
// than hand-authoring its own deploy machinery.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	ci: {
		deploy: {
			reusableWorkflow:    "kolohelios/kolohelios/.github/workflows/cf-deploy.yml@main"
			previewScriptPrefix: "buzzingo"
		}
	}
}
