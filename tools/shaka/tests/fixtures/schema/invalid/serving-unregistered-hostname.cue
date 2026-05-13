package project

// Rejected because `serving.hostnames` references a hostname not in
// the registry. Mirrors the deploy-side constraint via
// `domain.#KnownHostnames`; catches typos at `cue vet` time.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	serving: [
		{
			via:       "cloudflare-worker"
			hostnames: ["not-registered.example"]
		},
	]
}
