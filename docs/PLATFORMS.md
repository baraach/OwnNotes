# Platform layout

The note engine, storage, sync, commands and most of the interface are
shared. Only the pieces that differ per operating system are split out, so
a platform bug can be fixed in one file without risking the others.

## Rust

```
core/                         shared: notes, files, sync, crypto, importers
app-tauri/src-tauri/src/
  lib.rs                      shared Tauri builder, state and commands
  main.rs                     desktop entry
  platform/
    macos.rs                  macOS windows, printing, open/reveal, clipboard
    windows.rs                Windows windows, printing, open/reveal, clipboard
    linux.rs                  Linux windows, printing, open/reveal, clipboard
    ios.rs                    iOS sandbox data path and mobile stubs
    android.rs                Android sandbox data path and mobile stubs
  extras.rs, importer.rs, sync.rs
```

Each platform has its own complete `platform/*.rs` file; a platform fix
touches only that file. Shared note and sync code stays in `core/lib.rs`. A
shared note/format/sync bug belongs in `core/` or the shared part of
`lib.rs`.

## Interface

```
app-tauri/ui/
  index.html                  shared markup
  app.css                     shared tokens, layout and desktop panes
  platform.css                shared engine fallbacks (no blur, high contrast)
  mobile.css                  shared phone/tablet layout
  platform/macos.css          macOS-only corrections
  platform/windows.css        Windows title-bar/toolbar corrections
  platform/linux.css          Linux title-bar/toolbar corrections
  platform/ios.css            iOS-only corrections
  platform/android.css        Android status-bar corrections
  app.js                      shared application logic
```

Everything in `mobile.css` and `platform/*.css` is scoped with
`data-mobile` or `data-platform`, so the desktop layout does not match it.

## Tauri configuration

```
tauri.conf.json               shared defaults
tauri.macos.conf.json         macOS window chrome and minimum version
tauri.windows.conf.json       Windows bundle target
tauri.linux.conf.json         Linux bundle targets
tauri.ios.conf.json           iOS minimum version
tauri.android.conf.json       Android minimum SDK
```

The macOS window keys are also kept in the shared file because
`cargo run -p ownnotes-tauri` does not run the Tauri CLI's platform-config
merge. `cargo tauri build`/`dev` use the split files.

## Native projects

```
src-tauri/gen/apple/          Xcode project for iOS (build artifacts ignored)
src-tauri/gen/android/        Gradle project for Android (build artifacts ignored)
```

## Build

Desktop:

```bash
cargo run -p ownnotes-tauri
```

iOS (Xcode and CocoaPods required):

```bash
cd app-tauri/src-tauri
cargo tauri ios init --ci          # once
cargo tauri ios build --debug --target aarch64-sim --no-sign
# install the .app in gen/apple/build/arm64-sim on a simulator
```

Android (JDK 21, Android SDK 35 and NDK 27 required):

```bash
export JAVA_HOME="$(/usr/libexec/java_home -v 21)"
export ANDROID_HOME="$HOME/Library/Android/sdk"
export NDK_HOME="$ANDROID_HOME/ndk/27.0.12077973"
cd app-tauri/src-tauri
cargo tauri android init --ci      # once
cargo tauri android build --debug --target aarch64 --apk
```

Android release signing is optional. Create `gen/android/keystore.properties`
(ignored by git) with:

```properties
storeFile=/absolute/path/to/release.jks
storePassword=...
keyAlias=...
keyPassword=...
```

Then `cargo tauri android build --target aarch64 --apk` produces a signed
release APK under `gen/android/app/build/outputs/apk/universal/release/`.
Without that file the release build stays unsigned. Never commit the
keystore or its passwords.

iOS builds need a development team and signing identity before the
keychain entitlement can take effect; the simulator build is ad-hoc and
cannot store WebDAV credentials.

A shared UI smoke flow for both mobile targets lives in
`mobile-tests/smoke.yaml`:

```bash
maestro test --device <simulator-udid> mobile-tests/smoke.yaml
```

On this machine the SDK is installed at
`/opt/homebrew/share/android-commandlinetools` and the NDK at
`.../ndk/27.0.12077973`.
