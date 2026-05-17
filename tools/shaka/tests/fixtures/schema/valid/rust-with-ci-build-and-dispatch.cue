package project

#Project & {
	name: "blogctl"
	kind: "rust"
	coverage: {
		line: {fail:   1}
		branch: {fail: 1}
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
