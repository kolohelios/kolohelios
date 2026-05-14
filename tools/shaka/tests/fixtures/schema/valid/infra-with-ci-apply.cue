package project

#Project & {
	name: "cloudflare-dns"
	kind: "infra"
	ci: {
		apply: {
			reusable_workflow: "./.github/workflows/tf-apply.yml"
		}
	}
}
