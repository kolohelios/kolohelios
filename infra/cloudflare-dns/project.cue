package project

#Project & {
	name: "cloudflare-dns"
	kind: "infra"
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "cloudflare-dns"
			purpose: "Terraform remote state for the Cloudflare DNS zones (slice #1: kolohelios.com)"
		},
	]
	ci: apply: reusable_workflow: "./.github/workflows/tf-apply.yml"
}
