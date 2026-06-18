package project

// `platform` is constrained to "ios" | "android"; a native-app for any
// other platform has no defined host-build flow, so the schema rejects it.
#Project & {
	name: "buzzingo-watch"
	kind: "native-app"
	nativeApp: {
		platform: "watchos"
		ffiCrate: "buzzingo-ffi"
		appId:    "com.buzzingo.app"
	}
}
