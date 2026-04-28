package project

#Project: {
	name: string & =~"^[a-z][a-z0-9-]*$"
	kind: "rust" | "infra"
	coverage?: {
		line?: {
			warn: number & >=0 & <=100
			fail: number & >=0 & <=100
		}
		branch?: {
			warn: number & >=0 & <=100
			fail: number & >=0 & <=100
		}
	}
}
