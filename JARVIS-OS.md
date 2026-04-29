# jarvis-os

OS Linux integrado y agéntico construido sobre [IronClaw](README.md).
Voz envolvente, HUD circular cyan, control profundo del sistema vía MCP, todo
local con fallback cloud para LLM y TTS.

> Proyecto de estudio personal. Ver `jarvis-os-spec-v0.2.docx.md` para la
> especificación completa (arquitectura, fases F1→F10, políticas, formatos
> de auditoría, paleta visual del HUD).

## Estado actual: F0 — ISO booteable

Esta rama (`jarvis-fedora-os`) contiene la base NixOS sobre la que se
construye la live ISO de pruebas. **No reemplaza macOS en el iMac**: la
imagen se flashea a un USB, se arranca con Option al encender, y el disco
interno queda intacto.

Estado por fase:

- **Pre-F0** ✓ — toolchain Rust + Nix package manager + devShell + IronClaw compila.
- **F0** (en curso) — flake con `nixosConfigurations.imac-2014` y output `packages.iso`.
- **F1+** — pendiente (engine + voz MVP + HUD básico, ver spec).

## Hardware

| Rol      | Equipo                | Specs                                                |
|----------|-----------------------|------------------------------------------------------|
| Dev      | Bestia local          | i9-14900K (32 threads), RTX 4090 24 GB, 62 GB RAM, Ubuntu 24.04 |
| Target   | iMac 27" 5K Retina late 2014 | i7-4790K (8 threads), AMD R9 M295X 4 GB, 32 GB DDR3-1600 |

El dev box genera la ISO. El iMac sólo la consume vía USB.

## Estructura de archivos NixOS

```
flake.nix              # entry point: inputs, devShells, nixosConfigurations, packages.iso
nixos/
  imac-2014.nix        # módulo hardware: kernel, broadcom_sta, amdgpu, applesmc, audio
  desktop.nix          # Hyprland + greetd autologin + paquetes Wayland base
  iso.nix              # iso-image.nix wrapper + makeUsb/EfiBootable + volumeID
```

Todos los archivos `.nix` están comentados línea a línea para servir de
material de estudio. La pedagogía está integrada en los comentarios — al
modificar algo, leerse el bloque relevante primero.

## Stack técnico (decisiones cerradas)

| Capa         | Elección                                  | Razón                                              |
|--------------|-------------------------------------------|----------------------------------------------------|
| Distro base  | NixOS unstable                            | Declarativo, reproducible, ISO trivial vía flake.  |
| Compositor   | Hyprland                                  | wlroots → layer-shell nativo (HUD envolvente F2+). |
| Persistencia | **Stateless** (todo en RAM/tmpfs)         | Modo pruebas; persistencia volverá en futuro.      |
| Lenguaje núcleo | Rust 1.92                              | IronClaw, MSRV de Cargo.toml.                     |
| Voz STT      | Faster-Whisper distil-large-v3 (local)    | Ya consolidado, sin coste runtime.                |
| Voz TTS      | ElevenLabs Flash (cloud)                  | XTTS-v2 en CPU iMac no es viable.                 |
| LLM main     | Claude Sonnet 4.6 (cloud, prompt caching) | Coste razonable con caching, calidad alta.        |
| LLM fallback | Gemma 4 E4B (local, eventual)             | 4.5B params efectivos, 128K ctx, function calling. |
| Políticas    | Crate hermano `jarvis_policies` extiende `ironclaw_safety` | No OPA externo. |

## Comandos básicos

### Entrar al devShell

```bash
cd /home/nexus/git/jarvis-os
nix develop
```

Te entrega un shell con `rustc 1.92`, `cargo`, `pkg-config`, `cmake`, `git`,
`cacert` y `SSL_CERT_FILE` apuntando al bundle del Nix store.

### Compilar IronClaw

Dentro del devShell:

```bash
cargo check        # type-check rápido
cargo build        # build completo (debug)
cargo build --release  # build optimizado
```

### Construir la ISO

```bash
nix build .#iso
```

La primera vez descarga ~1-2 GB de paquetes desde caches. Output:
`result/iso/jarvis-os-*.iso`.

### Flashear el USB (≥ 4 GB)

> ⚠️ `dd` no perdona errores en el `of=`. Verificar el dispositivo antes con
> `lsblk`. **Equivocarse de letra borra el disco que NO querías borrar.**

```bash
lsblk                           # identifica el USB (p.ej. /dev/sdb)
sudo dd if=result/iso/jarvis-os-*.iso of=/dev/sdX bs=4M status=progress conv=fsync
sync
```

Alternativa más segura: `usbimager` o `balenaEtcher` (GUI con confirmación).

### Bootear en el iMac 2014

1. Insertar USB en el iMac apagado.
2. Encender manteniendo **Option (⌥)** pulsada.
3. En el boot picker de Apple, seleccionar la entrada **EFI Boot** (USB).
4. Esperar al systemd-boot/grub menú → entrar.
5. Auto-login al usuario `jarvis` → Hyprland.

Si no aparece la entrada EFI: revisar que la ISO se generó con
`makeEfiBootable = true` (debería; si falló, ver `nixos/iso.nix`).

## Notas sobre hardware del iMac 2014

| Componente | Driver / Módulo | Notas |
|-----|-----|-----|
| WiFi BCM4360 | `broadcom_sta` (alias `wl`) | Propietario, `unfree` + `insecure`. No hay alternativa libre que funcione. Permitido puntualmente en `imac-2014.nix`. |
| GPU R9 M295X (Tonga, GCN 1.2) | `amdgpu` | Soporte nativo desde kernel 4.x. |
| Audio Cirrus CS4208 | `snd_hda_intel` + PipeWire | Funciona out-of-the-box. |
| Sensores SMC | `applesmc` | Lectura de temperaturas y ventiladores. |
| Teclado Apple | `hid_apple` con `fnmode=2` | F-keys directas, multimedia con fn. |
| Cámara FaceTime HD | `facetimehd` (out-of-tree) | NO incluido todavía; añadir si se necesita. |

## Pendientes inmediatos (post-F0)

- [ ] Bootear la ISO en QEMU primero (`qemu-system-x86_64 -bios OVMF.fd`).
- [ ] Bootear en el iMac real, validar WiFi + GPU + audio + Hyprland.
- [ ] Añadir crate `crates/jarvis_policies/` (extensión de `ironclaw_safety`).
- [ ] Voice daemon Python (`voice_daemon/`) con openWakeWord + Silero VAD + Whisper.
- [ ] Linux MCP server Rust (`crates/jarvis_linux_mcp/`) con zbus + polkit + systemd.
- [ ] HUD Tauri (`crates/jarvis_hud/`) con layer-shell.

## Referencias

- Spec canónica: `jarvis-os-spec-v0.2.docx.md`
- IronClaw upstream (este mismo repo): [README.md](README.md)
- IronClaw ↔ OpenClaw paridad: [FEATURE_PARITY.md](FEATURE_PARITY.md)
- Hyprland docs: <https://wiki.hyprland.org/>
- NixOS manual: <https://nixos.org/manual/nixos/stable/>
- Determinate Nix: <https://docs.determinate.systems/>
