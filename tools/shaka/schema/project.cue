package project

#Namespace: {
	kind:    "tfstate" | "cache" | "assets"
	name:    string & =~"^[a-z][a-z0-9-]*$"
	purpose: string & !=""
}

#Project: {
	name: string & =~"^[a-z][a-z0-9-]*$"
	objectStorage?: {
		namespaces: [...#Namespace]
	}
} & ({
	kind: "rust"
	coverage: {
		line: {
			fail: number & >=0 & <=100
		}
		branch: {
			fail: number & >=0 & <=100
		}
	}
} | {
	kind: "infra"
} | {
	kind: "nix-lib"
})
