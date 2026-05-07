package project

#Project & {
	name: "devbox"
	kind: "infra"
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "DevBox"
			purpose: "uppercase rejected by kebab-case regex"
		},
	]
}
