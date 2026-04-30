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

## Deployment

The `arch/install.sh` recipe copies `ui/jarvis-os/` to `/usr/share/jarvis-os/qml/`
and enables `jarvis-ui.service` (systemd-user). The service launches:

```
quickshell -p /usr/share/jarvis-os/qml
```

Hotkeys (`Super+J`, `Super+Shift+J`) require the corresponding Hyprland keybinds
which are appended by install.sh to `arch/configs/hyprland/keybinds.conf`. The
keybinds invoke `scripts/jarvis-ui-toggle.sh` which writes to the UNIX socket
`/tmp/jarvis-ui.sock` listened by `Hotkeys.qml`.

## Architecture reference

See `docs/superpowers/specs/2026-04-30-jarvis-os-v0.3-ui-architecture-design.md`
and the implementation plan
`docs/superpowers/plans/2026-04-30-jarvis-os-v0.3-ui-architecture.md`.
