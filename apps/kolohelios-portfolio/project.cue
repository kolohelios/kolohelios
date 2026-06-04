package project

#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	worker: {
		// tailwindcss compiles the site's CSS; cue exports
		// data/work-history.cue to JSON the templates iterate over —
		// both invoked from wrangler.toml's [build] step.
		extraDevShellPackages: ["tailwindcss", "cue"]
	}
	audit: {
		overrides: [
			{
				rule:          "rust-has-tests"
				severity:      "off"
				justification: "Worker is a stub fallthrough; static assets serve all real content via wrangler [assets]. Meaningful tests land with dynamic paths (#193 contact form)."
			},
		]
	}
	serving: [
		{
			via:       "cloudflare-worker"
			hostnames: ["kolohelios.com"]
		},
	]
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "kolohelios.com"
		zone:         "kolohelios.com"
		cache: {
			bypassPaths: ["/api/"]
		}
	}
	ci: {
		deploy: {
			reusableWorkflow:    "./.github/workflows/cf-deploy.yml"
			previewScriptPrefix: "kolohelios-portfolio"
		}
	}
}
