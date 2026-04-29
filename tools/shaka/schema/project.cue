package project

#Project: {
	name: string & =~"^[a-z][a-z0-9-]*$"
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
