package project

// Worker that cross-compiles the same crate to non-wasm targets via
// `extraRustTargets` — the toolchain unions these onto the fixed
// `wasm32-unknown-unknown` (the buzzingo `uniffi` FFI shape).
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {
		extraRustTargets: [
			"aarch64-apple-ios",
			"aarch64-apple-ios-sim",
			"aarch64-linux-android",
		]
	}
}
