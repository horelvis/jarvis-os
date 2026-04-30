# jarvis-os UI · Quickshell shell

Quickshell QML files for the jarvis-os desktop UI.

## Run in dev

From the repo root:

```
quickshell -p ui/jarvis-os
```

Quickshell hot-reloads QML files on save.

## Structure

- `shell.qml` — entry point; instantiates each surface (layer-shell window).
- `theme/Palette.qml` — visual constants (singleton).
- `core/EventBus.qml` — WebSocket client for the IronClaw gateway.
- `core/IronclawRest.qml` — REST helpers for snapshot data.
- `core/Hotkeys.qml` — UNIX socket listener for hotkey IPC from Hyprland.
- `surfaces/*.qml` — one layer-shell window each (orb + 3 widgets).
- `components/*.qml` — reusable visual building blocks.

## Dependencies

- Quickshell (AUR `quickshell` or `quickshell-git`)
- Qt 6: base, declarative, websockets
- Wayland compositor with `wlr-layer-shell` support (Hyprland, sway, river)

## Architecture reference

See `docs/superpowers/specs/2026-04-30-jarvis-os-v0.3-ui-architecture-design.md`.
