package project

#Project & {
	name: "cloudflare-tokens"
	kind: "infra"
	infra: {
		description: "kolohelios — Cloudflare API tokens (Terraform + 1Password)"
		devShellPackages: ["opentofu", "_1password-cli"]
	}
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "cloudflare-tokens"
			purpose: "Terraform remote state for Cloudflare API tokens (per-token resources + 1Password sync)"
		},
	]
	ci: apply: reusable_workflow: "./.github/workflows/tf-apply.yml"
}
