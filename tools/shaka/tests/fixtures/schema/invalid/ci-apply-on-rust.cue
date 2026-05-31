package project

// ci.apply is only meaningful for infra projects (which delegate to
// the reusable tf-apply workflow); putting it on a rust-cli crate
// should fail the kind-specific schema branch.
#Project & {
	name: "shaka"
	kind: "rust-cli"
	cli: {
		binaryName: "shaka"
	}
	coverage: {
		line: {fail:   30}
		branch: {fail: 20}
	}
	ci: {
		apply: {
			reusable_workflow: "./.github/workflows/tf-apply.yml"
		}
	}
}
