package project

#Project & {
	name: "uptime"
	kind: "infra"
	infra: {
		description: "kolohelios — external uptime monitoring (Better Stack, Terraform)"
		devShellPackages: ["opentofu", "_1password-cli"]
	}
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "uptime"
			purpose: "Terraform remote state for the Better Stack uptime monitors (external probe for kolohelios.com; #196)"
		},
	]
	ci: apply: reusable_workflow: "./.github/workflows/tf-apply.yml"
}
