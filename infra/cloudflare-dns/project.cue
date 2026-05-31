package project

#Project & {
	name: "cloudflare-dns"
	kind: "infra"
	infra: {
		description: "kolohelios — Cloudflare DNS zones (Terraform)"
		devShellPackages: ["opentofu", "_1password-cli"]
	}
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "cloudflare-dns"
			purpose: "Terraform remote state for the Cloudflare DNS zones (slice #1: kolohelios.com)"
		},
	]
	ci: apply: reusable_workflow: "./.github/workflows/tf-apply.yml"
}
