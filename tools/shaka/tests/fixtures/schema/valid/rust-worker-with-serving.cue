package project

// Worker app with a `serving:` block — registered hostname passes
// the `domain.#KnownHostnames` constraint.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	worker: {}
	serving: [
		{
			via:       "cloudflare-worker"
			hostnames: ["kolohelios.com"]
		},
	]
}
