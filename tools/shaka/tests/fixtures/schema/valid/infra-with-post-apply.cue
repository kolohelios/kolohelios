package project

#Project & {
	name: "token-fixture"
	kind: "infra"
	ci: apply: {
		reusable_workflow: "./.github/workflows/tf-apply.yml"
		post_apply_command: "shaka token cloudflare verify --registry-dir tokens"
	}
}
