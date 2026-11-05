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
  app.css                     shared tokens, components and base panes
  platform.css                shared engine fallbacks (no blur, high contrast)
  platform/macos.css          complete macOS design and chrome
  platform/windows.css        complete Windows 11 (Fluent) design and chrome
  platform/linux.css          complete GNOME (Adwaita) design and chrome
  platform/ios.css            complete iOS phone/tablet design and chrome
  platform/android.css        complete Android phone/tablet design and chrome
  app.js                      shared application logic
```

The three desktops are split the same way. Each stylesheet sits on the shared
base and is scoped with `data-platform`, so a change to one cannot restyle
another. macOS has a floating sidebar with the traffic lights inside it, and one
toolbar row of capsules across list and note. Its see-through window needs
Tauri's `macos-private-api` feature, so this build cannot go on the Mac App
Store. Windows is flat and Mica-toned, with
accent selection pills, small corners and Settings-app toggle cards. Linux is
solid throughout, as Adwaita asks, with a header bar, neutral selection, boxed
lists and GTK switches. All three dim their chrome when the window loses focus.

The main window on macOS and Windows 11 has frosted system material behind it
(`platform/*.rs`), showing in an 8px strip around the app, in the gaps between its
cards and in the sidebar. Each of those mixes in the theme's colour to keep text
readable. Linux has no standard blur, so its sidebar is solid and only the strip
and gaps are clear. Clear pixels come out black on a Linux desktop without a
compositor, so `platform/linux.rs` watches for one and the page paints them when
there is none. Filling the screen removes the strip. macOS keeps its traffic
lights. Windows and Linux have no system frame, so the toolbar draws their window
buttons.

There is deliberately no shared mobile stylesheet: the iOS and Android
layouts and chrome are independent, so a change to one cannot restyle the
other. Every phone rule is scoped with `data-mobile` and `data-platform`,
so the desktop layout never matches it.

Each file follows its own platform's conventions rather than a shared phone
layout. iOS has large titles, inset grouped lists, a floating glass tab bar
with Search apart from it, switches and a back swipe from the leading edge.
Android follows Material 3: a search bar at the top, a floating action button,
modal bottom sheets, ripples and tonal colours mixed from the theme's accent,
with a floating glass bottom bar and editor toolbar.

On both, the note list and Browse scroll behind the blurred bars, and a pill
slides under the active tab (`moveTabIndicator`). Each stylesheet ends with its
own motion section, which `prefers-reduced-motion` turns off.

Neither platform shows a screen as a pop-up. Settings is a tab that pushes one
page at a time, and Export, History and the welcome screen are full pages. Only
short questions (a system-style alert) and menus (an action or bottom sheet)
stay as overlays.

Android draws behind its system bars and does not resize the webview for the
keyboard, so `MainActivity.kt` reports both to the page through
`window.__applyAndroidInsets`. iOS follows `visualViewport` instead. Both end up
in `--keyboard-inset`, which shrinks the app so that the editor toolbar stays
above the keyboard.

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
