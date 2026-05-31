package project

#Project & {
	name: "blogctl"
	kind: "rust-cli"
	cli: {
		binaryName: "blogctl"
	}
	coverage: {
		line: {fail:   1}
	}
	ci: {
		build: {
			filterKey:   "blogctl"
			jobId:       "blogctl"
			displayName: "blogctl"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/blogctl"
				visibility: "public"
				rolling:    true
			}
			dispatch: [
				{
					repo:      "kolohelios/blogs-and-posts"
					eventType: "blogctl-published"
				},
			]
		}
	}
}
