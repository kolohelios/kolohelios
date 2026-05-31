package project

#Project & {
	name: "cloudflare-deploy"
	kind: "infra"
	infra: {
		description: "kolohelios — Cloudflare deploy attachments (Terraform, generated from project.cue)"
		devShellPackages: ["opentofu", "_1password-cli"]
	}
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "cloudflare-deploy"
			purpose: "Terraform remote state for the generated Cloudflare deploy attachments (Worker custom domains, future targets)"
		},
	]
	ci: apply: reusable_workflow: "./.github/workflows/tf-apply.yml"
}
