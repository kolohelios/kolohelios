package project

// A SwiftUI/iOS native app shell consuming a uniffi FFI crate's
// bindings. Host-built (Xcode); shaka validates the project.cue but
// generates no flake/justfile and never builds it.
#Project & {
	name: "buzzingo-ios"
	kind: "native-app"
	nativeApp: {
		platform: "ios"
		ffiCrate: "buzzingo-ffi"
		appId:    "com.buzzingo.app"
	}
}
