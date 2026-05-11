package project

// Worker app with no opt-in coverage block and no deploy block yet.
// Coverage is optional on rust-worker since cargo-llvm-cov can't
// measure the wasm-target code; deploy is optional until the app is
// ready to be attached.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
}
