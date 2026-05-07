package project

#Project & {
	name: "devbox"
	kind: "infra"
	objectStorage: namespaces: [
		{
			kind:    "secrets"
			name:    "devbox"
			purpose: "not a registered kind"
		},
	]
}
