package labels

#LabelSet & {
	labels: [
		// Scope labels — every open issue must carry at least one,
		// per `shaka issue audit`.
		{name: "aof", color:          "d93f0b", description: "Personal-OS / areas tree tooling", scope: true},
		{name: "blogctl", color:      "1f77b4", description: "blogctl CLI / blog post workflow tooling", scope: true},
		{name: "ci", color:           "fbca04", description: "CI pipeline / GitHub Actions / preflight gates", scope: true},
		{name: "claude", color:       "d4c5f9", description: "claude-code config, hooks, settings", scope: true},
		{name: "infra/devbox", color: "0052cc", description: "Devbox / dev-environment infrastructure", scope: true},
		{name: "infra/home", color:   "006b75", description: "Home-manager / personal workstation infrastructure", scope: true},
		{name: "meta", color:         "ededed", description: "Cross-cutting repo / monorepo policy issues", scope: true},
		{name: "nix", color:          "5277c3", description: "Nix flakes, kolohelios-nix shared lib", scope: true},
		{name: "pollen-alert", color: "0e8a16", description: "Pollen alert worker", scope: true},
		{name: "portfolio", color:    "1d76db", description: "Personal portfolio site at kolohelios.com", scope: true},
		{name: "shaka", color:        "5319e7", description: "shaka CLI / repo tooling", scope: true},
		{name: "todoist", color:      "e60023", description: "Todoist CLI tool", scope: true},

		// Type / housekeeping / category labels — not scope labels.
		{name: "bug", color:              "d73a4a", description: "Something isn't working", scope: false},
		{name: "dependencies", color:     "0366d6", description: "Pull requests that update a dependency file", scope: false},
		{name: "documentation", color:    "0075ca", description: "Improvements or additions to documentation", scope: false},
		{name: "duplicate", color:        "cfd3d7", description: "This issue or pull request already exists", scope: false},
		{name: "enhancement", color:      "a2eeef", description: "New feature or request", scope: false},
		{name: "github_actions", color:   "000000", description: "Pull requests that update GitHub Actions code", scope: false},
		{name: "good first issue", color: "7057ff", description: "Good for newcomers", scope: false},
		{name: "help wanted", color:      "008672", description: "Extra attention is needed", scope: false},
		{name: "invalid", color:          "e4e669", description: "This doesn't seem right", scope: false},
		{name: "question", color:         "d876e3", description: "Further information is requested", scope: false},
		{name: "rust", color:             "000000", description: "Pull requests that update rust code", scope: false},
		{name: "wontfix", color:          "ffffff", description: "This will not be worked on", scope: false},
	]
}
