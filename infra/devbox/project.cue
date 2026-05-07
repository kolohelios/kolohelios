package project

#Project & {
	name: "devbox"
	kind: "infra"
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "devbox"
			purpose: "Terraform remote state for the devbox Linode infrastructure"
		},
	]
}
