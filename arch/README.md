# jarvis-os sobre Arch Linux

Workflow de instalación + mantenimiento de jarvis-os usando Arch Linux como
base. Reemplaza el setup NixOS de las versiones v1-v19 (preservado en `legacy/`).

## Instalación inicial

**Precondición**: Arch Linux instalado vía `archinstall` o método manual,
booteando ya en sistema persistente (no live USB).

```bash
# 1. Clonar el repo de jarvis-os (puede ser /opt/, ~/code/, donde prefieras)
sudo mkdir -p /opt && sudo chown $USER:$USER /opt
git clone <url-de-tu-repo-jarvis-os> /opt/jarvis-os
cd /opt/jarvis-os
git checkout jarvis-arch-os

# 2. Bootstrap completo
./arch/install.sh
```

`install.sh` es **idempotente** (re-ejecutar es seguro). Hace:
1. Instala dependencias pacman base.
2. Instala paru (AUR helper).
3. Clona end-4/dots-hyprland (no ejecuta su setup — eso lo haces tú post-script).
4. Compila los crates Rust (`jarvis_linux_mcp`).
5. Instala binarios en `/usr/local/bin/`.
6. Configura systemd-user services.
7. Configura snapper para snapshots Btrfs (rollback equivalente a NixOS generations).
8. Crea plantilla de `~/.ironclaw/.env`.

**Pasos manuales tras el script**:
1. `cd ~/dots-hyprland && ./setup install` — instala el shell illogical-impulse.
2. Edita `~/.ironclaw/.env` con tus API keys reales (Anthropic mínimo).
3. `systemctl --user enable --now jarvis-mcp-register.service`.
4. Logout + login para entrar a Hyprland con jarvis-os.

## Verificación

```bash
ironclaw mcp test jarvis-linux   # debería listar 8 tools
jarvis-chat run                   # arranca el agente en REPL
```

## Aplicar cambios (≈ "nixos-rebuild switch")

Cuando edites código en los crates Rust o configs en `arch/configs/`:

```bash
cd /opt/jarvis-os
git pull                # opcional: bajar cambios remotos
./arch/update.sh        # aplica cambios al sistema instalado
```

`update.sh` recompila lo que cargo decida (caché incremental), reinstala
binarios solo si su sha256 cambió, sincroniza configs/systemd-user/wallpaper,
hace daemon-reload si units cambiaron, y verifica integridad con
`ironclaw mcp test`.

## System updates (Arch base)

```bash
sudo pacman -Syu        # snapper auto-snapshot pre/post
```

Si algo se rompe tras el update → boot picker GRUB muestra entries de
snapshots → eliges el anterior. Equivalente funcional a `nixos-rebuild --rollback`.

## Estructura del directorio `arch/`

```
arch/
├── README.md              # este archivo
├── install.sh             # instalación inicial
├── update.sh              # apply changes (≈ nixos-rebuild switch)
├── configs/
│   └── hyprland/
│       └── jarvis-os.conf # overrides custom encima de illogical-impulse
├── scripts/
│   └── jarvis-chat        # wrapper que sourcea ~/.ironclaw/.env y exec ironclaw
├── systemd-user/
│   └── jarvis-mcp-register.service  # registra jarvis-linux MCP en cada boot
└── templates/
    └── ironclaw.env       # plantilla de .env (sin keys reales)
```

## Mapeo conceptual NixOS → Arch

| Concepto NixOS | Equivalente Arch |
|---|---|
| `flake.nix` + `nixos/*.nix` | `arch/install.sh` + `arch/configs/` |
| `nixos-rebuild switch` | `./arch/update.sh` |
| `system.activationScripts` | bash en `install.sh`/`update.sh` |
| Generations + boot menu rollback | snapper + grub-btrfs |
| `home-manager` modules | edición directa en `~/.config/` |
| `programs.hyprland.enable = true` | `pacman -S hyprland` |
| Reproducibilidad bit-exacta | versionado git + lockfile (Cargo.lock) |

## Si se quiere live USB en el futuro

Cuando jarvis-os tenga una versión estable validada en daily-use, se puede
generar un live USB ISO via `archiso` (custom releng profile que pre-instala
todo). Eso vendría en una vXX futura, no es prioridad para v20.0.

## Migración de vuelta a NixOS

El trabajo NixOS v1-v19 está intacto en `legacy/nixos/` + `legacy/flake.nix`.
Si en algún momento quieres volver, los crates Rust son idénticos y solo
hace falta restaurar el packaging Nix-specific.
