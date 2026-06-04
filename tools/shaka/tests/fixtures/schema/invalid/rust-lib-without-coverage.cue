package project

// rust-lib requires a coverage block (a shared lib is pure, natively
// testable logic). Omitting it must fail cue vet.
#Project & {
	name: "missing-coverage-lib"
	kind: "rust-lib"
}
