package project

// A consumer whose production custom_domain is a wrangler route isolates
// it under `[env.production]` and selects that environment on the real
// deploy, keeping route-free previews on *.workers.dev.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	ci: {
		deploy: {
			reusableWorkflow:    "kolohelios/kolohelios/.github/workflows/cf-deploy.yml@main"
			previewScriptPrefix: "buzzingo"
			environment:         "production"
		}
	}
}
