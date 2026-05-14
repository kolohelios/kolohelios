package project

#Project & {
	name: "cloudflare-deploy"
	kind: "infra"
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "cloudflare-deploy"
			purpose: "Terraform remote state for the generated Cloudflare deploy attachments (Worker custom domains, future targets)"
		},
	]
	ci: apply: reusable_workflow: "./.github/workflows/tf-apply.yml"
}
