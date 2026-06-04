package project

#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	worker: {}
	deploy: {
		target:       "fly"
		customDomain: "kolohelios.com"
		zone:         "kolohelios.com"
	}
}
