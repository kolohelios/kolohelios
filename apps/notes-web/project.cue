package project

#Project & {
	name: "notes-web"
	kind: "rust-worker"
	audit: {
		overrides: [
			{
				rule:          "rust-has-tests"
				severity:      "off"
				justification: "Worker is a 404 stub fallthrough; the HTMX shell and the notes-editor bundle serve all content via wrangler [assets]. The editing/sync logic lives in apps/notes-editor (native-tested) and notes-sync."
			},
		]
	}
}
