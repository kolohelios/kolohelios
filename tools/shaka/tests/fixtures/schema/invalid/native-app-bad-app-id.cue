package project

// `appId` must be reverse-DNS (lowercase dot-separated segments). A
// single-segment, capitalized, or space-bearing value isn't a valid
// bundle id / applicationId, so the schema rejects it.
#Project & {
	name: "buzzingo-ios"
	kind: "native-app"
	nativeApp: {
		platform: "ios"
		ffiCrate: "buzzingo-ffi"
		appId:    "Buzzingo"
	}
}
