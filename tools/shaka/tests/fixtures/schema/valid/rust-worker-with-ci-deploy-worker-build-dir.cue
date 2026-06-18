package project

// A workspace Worker builds from a member crate, so it declares
// `workerBuildDir` — threaded to cf-deploy.yml's `worker_build_dir` input.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	ci: {
		deploy: {
			reusableWorkflow:    "kolohelios/kolohelios/.github/workflows/cf-deploy.yml@main"
			previewScriptPrefix: "buzzingo"
			environment:         "production"
			workerBuildDir:      "crates/buzzingo-server"
		}
	}
}
