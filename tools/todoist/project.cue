package project

#Project & {
	name: "todoist"
	kind: "rust-cli"
	cli: {
		binaryName: "todoist"
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
}
