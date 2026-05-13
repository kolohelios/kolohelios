package project

// Rejected because `deploy.zone` is required and the regex-based
// `#KnownHostnames` constraint has no concrete default (a singleton
// enum would default to its only value; the regex disjunction
// doesn't).
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "kolohelios.com"
	}
}
