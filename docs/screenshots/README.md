# Screenshots

| File | What it shows |
|---|---|
| `app.png` | Desktop: the three-pane window with the same sample notes as the phones |
| `ios.png` | iPhone: large-title list, floating navigation and Search action |
| `android.png` | Android: Material 3 search bar, note list and bottom navigation |

The three files above are the ones used in the main README.

## Capturing

- Desktop: capture the OwnNotes window and save it as `app.png`.
- iOS simulator: `xcrun simctl io booted screenshot docs/screenshots/ios.png`
- Android emulator: `adb exec-out screencap -p > docs/screenshots/android.png`

Keep the note list at the top and avoid test notes when refreshing these images.
