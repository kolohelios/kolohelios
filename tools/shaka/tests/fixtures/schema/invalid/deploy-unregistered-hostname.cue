package project

// Rejected because `customDomain` references a hostname not in the
// registry (`infra/cloudflare-dns/domains/`). CUE constrains the
// field to `domain.#KnownHostnames`; a typo fails `cue vet` before
// any TF apply.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "not-registered.example"
		zone:         "kolohelios.com"
	}
}
