package project

#Project & {
	name: "shaka"
	kind: "infra"
	audit: {
		overrides: [
			{
				rule:          "readme-present"
				severity:      "off"
				justification: ""
			},
		]
	}
}
