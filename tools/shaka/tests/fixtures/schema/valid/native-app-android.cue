package project

// A Jetpack Compose/Android native app shell consuming the same uniffi
// FFI crate. Host-built (Gradle + Android SDK); the one native-app kind
// covers both platforms via the `platform` field.
#Project & {
	name: "buzzingo-android"
	kind: "native-app"
	nativeApp: {
		platform: "android"
		ffiCrate: "buzzingo-ffi"
		appId:    "com.buzzingo.app"
	}
}
