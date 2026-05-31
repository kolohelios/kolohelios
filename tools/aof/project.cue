package project

#Project & {
	name: "aof"
	kind: "rust-cli"
	cli: {
		binaryName: "aof"
		// devShell-only project today; no `nix build ./tools/aof`
		// consumer, so the package emission is off.
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
