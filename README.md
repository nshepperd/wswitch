# wswitch

A small window switcher for macOS, in the spirit of
[AltTab](https://github.com/lwouis/alt-tab-macos), written in Rust with
**zero dependencies**: no crates, just the system frameworks via a few hundred
lines of hand-written FFI. The whole app is a ~400 KB binary.

- Live previews of every window, not just every app
- Minimized windows and windows of hidden apps are included (dimmed)
- Simple HUD look: one blurred panel, rounded tiles, icon and title
- Works on whichever display the mouse is on, over full-screen apps too

## Usage

| Keys                           | Action                           |
| ------------------------------ | -------------------------------- |
| hold <kbd>⌥</kbd>, press <kbd>⇥</kbd> | open the switcher / next window |
| <kbd>⇧</kbd><kbd>⇥</kbd>, <kbd>←</kbd> <kbd>→</kbd> | previous / next             |
| release <kbd>⌥</kbd>, <kbd>↩</kbd>    | switch to the selected window |
| <kbd>esc</kbd>, click outside   | cancel                           |
| click a preview                | switch to it (hovering rings it, without moving the keyboard selection) |

Quit from the menu bar icon.

## Build and install

You need Rust (`curl https://sh.rustup.rs -sSf | sh`) and the Xcode command
line tools.

```sh
make app       # builds target/wswitch.app (ad-hoc signed)
make install   # copies it to /Applications
make run       # rebuilds and relaunches
```

On first launch macOS asks for two permissions, both in **System Settings →
Privacy & Security**:

- **Accessibility**: required to catch the shortcut and focus windows.
- **Screen & System Audio Recording**: for previews and window titles.
  Without it you get app icons instead of previews. Relaunch wswitch after
  granting it.

To start it at login, add it under **System Settings → General → Login Items**.

> **Rebuilding:** macOS ties permissions to the app's signature, and an
> ad-hoc signature changes with every build. Run `make cert` once to create a
> self-signed signing certificate in your login keychain; `make app` then uses
> it automatically and permissions survive rebuilds. (It's only for your Mac;
> delete "wswitch-local" in Keychain Access to remove it.)

## Settings

Settings are plain user defaults; quit and relaunch after changing them.

```sh
# Use ⌘⇥ instead of ⌥⇥ (the system app switcher is turned off while wswitch runs)
defaults write io.github.wswitch Modifier command    # or: option, control

# Hide the menu bar icon (quit with: pkill wswitch)
defaults write io.github.wswitch HideMenuBarIcon -bool true
```

## How it works

| File             | What it does |
| ---------------- | ------------ |
| `src/main.rs`    | App setup, a `CGEventTap` that handles all keyboard and mouse input, and the open/step/commit state machine |
| `src/windows.rs` | Lists windows (`CGWindowListCopyWindowInfo` cross-checked against Accessibility), captures previews, focuses windows |
| `src/ui.rs`      | A non-activating `NSPanel` with an `NSVisualEffectView`; tiles are Core Animation layers, laid out in centered rows |
| `src/ffi.rs`     | Bindings: a `msg!` macro over `objc_msgSend`, CoreFoundation, CoreGraphics, Accessibility, libdispatch |

Because the event tap handles all input, the panel never takes focus. Previews
are captured on a background thread and cached, so reopening the switcher
shows the last previews right away while fresh ones load.

Like AltTab, wswitch uses a few private SkyLight calls (looked up at runtime)
for fast window capture and for focusing one specific window rather than the
whole app. If they're ever missing, it falls back to public APIs.

## Limitations

- Shows windows on the current Space only (plus minimized and hidden ones).
- When a secure text field has focus (e.g. a password prompt), macOS hides
  keystrokes from event taps, so the shortcut won't fire.

## License

MIT
