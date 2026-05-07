package project

#Project & {
	name: "asset-host"
	kind: "infra"
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "asset-host"
			purpose: "Terraform remote state for the asset host"
		},
		{
			kind:    "assets"
			name:    "asset-host"
			purpose: "Static assets served from the bucket"
		},
	]
}
