package project

#Project & {
	name: "todoist"
	kind: "rust-cli"
	cli: {
		binaryName:       "todoist"
		shellCompletions: true
		// No `nix build ./tools/todoist` consumer today; only the
		// devShell is needed. Skipping the package keeps the
		// generated flake minimal.
		package: false
	}
	coverage: {
		line: {
			fail: 1
		}
		branch: {
			fail: 0
		}
	}
	audit: {
		overrides: [
			{
				rule:          "rust-cli-package-false-requires-override"
				severity:      "off"
				justification: "no `nix build ./tools/todoist` consumer today; only the devShell is needed, so neither `cli.package` nor `ci.build` are wired"
			},
		]
	}
}
