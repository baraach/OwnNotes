# Screenshots

| File | What it shows |
|---|---|
| `macos.png` | macOS: floating sidebar, traffic lights and a rich note open |
| `windows.png` | Windows 11: Fluent toolbar, notebook tree and a rich note open |
| `linux.png` | Linux: Adwaita header bar, notebook tree and a rich note open |
| `ios.png` | iPhone: large-title list, floating navigation and Search action |
| `android.png` | Android: Material 3 search bar, note list and bottom navigation |

The five files above are the ones used in the main README.

## Capturing

- macOS, Windows and Linux: capture the OwnNotes window only and save it as
  `macos.png`, `windows.png` or `linux.png`.
- iOS simulator: `xcrun simctl io booted screenshot docs/screenshots/ios.png`
- Android emulator: `adb exec-out screencap -p > docs/screenshots/android.png`

Keep the note list at the top and use the sample notes, not test notebooks,
when refreshing these images.
