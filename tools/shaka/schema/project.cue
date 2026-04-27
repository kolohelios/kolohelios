package project

#Project: {
	name: string & =~"^[a-z][a-z0-9-]*$"
	kind: "rust" | "infra"
}
