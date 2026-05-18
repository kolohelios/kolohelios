package project

// ci.deploy is only valid on rust-worker (its body wraps cf-deploy.yml,
// which only makes sense for Cloudflare Worker projects); rejecting
// the field on plain rust crates keeps the schema honest.
#Project & {
	name: "shaka"
	kind: "rust"
	coverage: {
		line: {fail:   30}
		branch: {fail: 20}
	}
	ci: {
		deploy: {
			reusableWorkflow:    "./.github/workflows/cf-deploy.yml"
			previewScriptPrefix: "shaka"
		}
	}
}
