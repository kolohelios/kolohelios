package project

#Project & {
	name: "cloudflare-portfolio"
	kind: "infra"
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "cloudflare-portfolio"
			purpose: "Terraform remote state for the Cloudflare Pages project serving the portfolio (slice #2 of #186)"
		},
	]
}
