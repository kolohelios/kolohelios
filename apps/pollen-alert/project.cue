package project

#Project & {
	name: "pollen-alert"
	kind: "rust-worker"
	audit: {
		overrides: [
			{
				rule:          "rust-has-tests"
				severity:      "off"
				justification: "Scaffold stub; meaningful tests land with the scoring module (#405)."
			},
		]
	}
}
