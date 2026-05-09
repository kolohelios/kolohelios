package project

#Project & {
	name: "cloudflare-tokens"
	kind: "infra"
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "cloudflare-tokens"
			purpose: "Terraform remote state for Cloudflare API tokens (per-token resources + 1Password sync)"
		},
	]
}
