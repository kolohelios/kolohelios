package project

#Project & {
	name: "devbox"
	kind: "infra"
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "devbox"
			purpose: ""
		},
	]
}
