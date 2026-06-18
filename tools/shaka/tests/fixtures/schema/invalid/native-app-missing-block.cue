package project

// `native-app` requires a `nativeApp` block (platform, ffiCrate, appId);
// declaring the kind without it leaves the app's platform and provenance
// undefined, so the schema rejects it.
#Project & {
	name: "buzzingo-ios"
	kind: "native-app"
}
