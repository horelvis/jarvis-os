> **OBSOLETED 2026-04-30** — Esta spec proponía un extension layer en bash + manifest TOML con motor `apply.sh`. Quedó obsoleta cuando el scope cambió de "OS con capas externas" a "UI sobre IronClaw como motor". Reemplazada por `2026-04-30-jarvis-os-v0.3-ui-architecture-design.md`. Conservada como referencia histórica.

---

# jarvis-os extension layer — design

**Date:** 2026-04-29
**Branch:** `jarvis-arch-os`
**Status:** approved (brainstorming) → ready to plan

## Contexto y problema

Hoy, añadir cualquier componente nuevo a jarvis-os (un crate Rust, un MCP
server adicional, un Hyprland override, una integración) toca **6-8 archivos
distintos**:

- `Cargo.toml` workspace (line con el path al crate)
- `arch/install.sh` → deps pacman
- `arch/install.sh` → build/install del binary
- `arch/update.sh` → build/install/restart del binary
- `arch/systemd-user/<unit>.service`
- `arch/templates/ironclaw.env` (vars que la extensión necesita)
- A veces `arch/scripts/` (wrapper) o `arch/configs/<software>/` (overrides)

Cada vez que pivotamos (Python voice_daemon → Rust voice_daemon, añadir
PipeWire echo-cancel, añadir un crate más) la fricción reaparece y la
fuente de verdad de "qué hace falta para que la extensión funcione" queda
**dispersa** en bash imperativo.

Adicionalmente: el ciclo `editar Rust → ver el cambio en Asus` es lento
porque `cargo build --release` corre en el laptop ULV (10-20 min al primer
build, 1-3 min incremental). Tenemos un dev box i9-14900K que podría hacer
ese build en 30 segundos, pero hoy no hay infraestructura para eso.

## Objetivos

1. Añadir o modificar una extensión toca **solo su carpeta** (`extensions/<name>/`).
2. La fuente de verdad es un **manifest TOML declarativo** por extensión.
3. `install.sh` y `update.sh` se simplifican a orquestadores que descubren
   manifests y aplican lo que dicen.
4. El ciclo "editar en dev box → ver en Asus" baja a **<60 segundos** vía
   `deploy-from-dev.sh` que cross-compila local y rsynchea binarios.
5. Migración incremental: cada paso de migración deja el sistema funcional;
   ninguno requiere un big-bang.

## No-objetivos (v1)

- **Marketplace de extensiones de terceros** (publicación, firma, sandbox) —
  fuera de scope. Las extensiones son código nuestro o de gente con commit
  access al repo.
- **CLI runtime tipo `jarvis-extensions list/enable foo`** — interesante pero
  no resuelve el dolor actual ("instalar una extensión nueva"); evolución
  natural si hace falta más adelante.
- **Hyprland custom files, scripts wrappers en `/usr/local/bin/`, registro
  MCP automático, hooks pre/post** — fuera de v1. Se añaden cuando aparezca
  el segundo caso real que los justifique.
- **Versionado y resolución de deps entre extensiones** — todas las
  extensiones son independientes en v1. Si surge necesidad: `[meta]
  depends_on = [...]`.
- **Tests automáticos formales del applier** — `apply.sh --dry-run` y
  `--check` cubren v1; bats / sandbox queda para v2.

## Arquitectura

```
/opt/jarvis-os/
├── arch/
│   ├── install.sh              # bootstrap base (deps Arch, paru, snapper) → exec apply
│   ├── update.sh               # exec apply
│   ├── apply.sh                # NEW: orquestador (descubre, parsea, aplica manifests)
│   ├── deploy-from-dev.sh      # NEW: corre en dev box; build local + rsync + ssh apply
│   ├── lib/manifest_get.py     # NEW: tomllib → JSON dump (~10 LoC)
│   ├── configs/                # core configs (hyprland, pipewire) — sin cambios
│   ├── systemd-user/           # solo units del core
│   └── templates/ironclaw.env  # vars del core (DB, ANTHROPIC_API_KEY)
├── crates/                     # SIN CAMBIOS (ironclaw, jarvis_*)
└── extensions/                 # NEW
    ├── jarvis-linux-mcp/
    │   ├── manifest.toml
    │   ├── systemd-user/jarvis-mcp-register.service
    │   └── env/mcp.env
    └── jarvis-voice-daemon/
        ├── manifest.toml
        ├── systemd-user/jarvis-voice-daemon.service
        └── env/voice.env
```

**Idea central**: una extensión es una carpeta self-contained con su
`manifest.toml` y sus assets locales (systemd units, env templates,
wallpaper, sonidos). Los crates Rust **siguen viviendo en `crates/`** del
workspace; el manifest los referencia por nombre (`crate =
"jarvis_voice_daemon"`), no por path.

`ironclaw` no es una extensión — es el core del sistema. Las extensiones
son lo que el sistema añade / extiende (MCP servers, voice daemon,
heads-up displays, etc.).

## Manifest format (`extensions/<name>/manifest.toml`)

Forma canónica completa con todas las secciones soportadas en v1.
Todas son **opcionales** salvo `[meta]`.

```toml
[meta]
name = "jarvis-voice-daemon"        # convención: igual que el nombre de la carpeta
version = "0.1.0"
description = "Cliente Rust de ElevenLabs Conversational AI"

# Cero o más binarios. Cada uno corresponde a un crate del workspace.
[[binary]]
crate = "jarvis_voice_daemon"       # nombre Cargo workspace member
bin = "jarvis-voice-daemon"          # nombre del bin output
install_to = "/usr/local/bin/"       # opcional, default /usr/local/bin/

# Deps de sistema. Se agregan con TODAS las extensiones antes de pacman.
[deps]
pacman = ["alsa-lib", "portaudio"]
aur = []                             # paru install si no vacío

# Cero o más systemd-user units. La ruta es relativa al manifest.
[[systemd.user]]
unit = "systemd-user/jarvis-voice-daemon.service"
enable_on_install = true             # systemctl --user enable --now
restart_on_update = true             # restart si el binary cambió

# Variables de entorno. El template se appendea a ~/.ironclaw/.env solo
# si no existe ya. required_keys valida que el user las haya rellenado.
[env]
template = "env/voice.env"
required_keys = ["ELEVENLABS_API_KEY", "ELEVENLABS_AGENT_ID"]

# Assets sueltos. Cada entrada copia src (relativo al manifest) a dst
# (absoluto, soporta ~ para HOME).
[[assets]]
src = "assets/wallpaper.jpg"
dst = "~/Pictures/jarvis-os/wallpaper.jpg"
```

Una extensión que sólo aporta un binario (sin systemd, sin env, sin
assets) es un manifest de 5 líneas:

```toml
[meta]
name = "alguna-tool"
version = "0.1.0"

[[binary]]
crate = "alguna_tool"
bin = "alguna-tool"
```

## Componentes

### `arch/lib/manifest_get.py` (~10 LoC)

```python
#!/usr/bin/env python3
import sys, tomllib, json
with open(sys.argv[1], 'rb') as f:
    print(json.dumps(tomllib.load(f)))
```

Convierte TOML a JSON. Bash usa `jq` (ya es dep) para extraer campos.
No mantenemos parser TOML propio.

### `arch/apply.sh` (~200 LoC)

Orquestador en bash. Funciones internas:

| Función | Qué hace |
|---|---|
| `discover()` | encuentra `extensions/*/manifest.toml`, devuelve lista de paths |
| `aggregate_pacman()` | reúne `[deps] pacman` de todos los manifests, hace `sudo pacman -S --needed` una sola vez |
| `aggregate_aur()` | igual con `[deps] aur` y `paru` |
| `cargo_build()` | `cargo build --release --bin <bin>` por cada `[[binary]]` de las extensiones (`ironclaw`, que es core, lo sigue compilando `install.sh` directamente) |
| `install_binary(manifest)` | sha256 check + `sudo install -Dm755` |
| `install_units(manifest)` | copia systemd unit + `daemon-reload` + enable según flags |
| `install_env(manifest)` | por cada línea `KEY=...` del template, append a `~/.ironclaw/.env` solo si esa KEY aún no aparece (idempotente, preserva ediciones del user); luego valida `required_keys` |
| `install_assets(manifest)` | copia src→dst si cambió (mtime/sha256) |
| `restart_units(manifest)` | restart units con `restart_on_update=true` cuando binary cambió |
| `apply_extension(path)` | orquesta lo de arriba para un manifest |

Flags CLI:
- `--dry-run`: hace los reads (parsea, calcula hashes, lista cambios) y log lo que haría sin ejecutar.
- `--check`: verifica integridad post-apply (bins en `/usr/local/bin/`, units activas, required_keys rellenas).
- `--skip-cargo-build`: usa los binarios ya en `target/release/` sin recompilar (deploy desde dev box).
- `--only=<name>`: aplica solo esa extensión (debug).

### `arch/deploy-from-dev.sh` (~50 LoC, corre en dev box)

```
JARVIS_TARGET=jarvis@asus.local ./arch/deploy-from-dev.sh
  ↓
  1. cargo build --release [bins declarados en manifests]    [~30s en i9]
  2. rsync target/release/<bins> arch/ extensions/ crates/ <target>:/opt/jarvis-os/
  3. ssh <target> 'cd /opt/jarvis-os && ./arch/apply.sh --skip-cargo-build'
```

Requiere SSH passwordless desde dev box a `$JARVIS_TARGET`. La key del dev
box debe estar en `~/.ssh/authorized_keys` del Asus (paso de setup
manual una vez).

## Flujos

### Install fresh (Arch recién instalado)

```
./arch/install.sh
  → pacman base-devel git rustup uv portaudio alsa-lib hyprland ...
  → rustup default stable
  → paru install
  → clone end-4/dots-hyprland
  → snapper config
  → exec ./arch/apply.sh

./arch/apply.sh
  → discover extensions/
  → aggregate pacman deps (de todas las extensions)
  → cargo build --release ironclaw                     # core
  → cargo build --release [bins de cada manifest]      # extensiones
  → install ironclaw a /usr/local/bin/                  # core
  → for ext in extensions:
       install_binaries / install_units / install_env / install_assets / restart_units
  → verify (ironclaw mcp test, etc.)
```

### Update local

```
git pull
./arch/update.sh   →   exec ./arch/apply.sh
```

Idempotente: cargo recompila solo lo cambiado, sha256 reinstall, units
solo se restartean si su binary cambió.

### Deploy remoto desde dev box

```
JARVIS_TARGET=jarvis@asus.local ./arch/deploy-from-dev.sh
  → cargo build --release  (rápido en i9)
  → rsync binaries + arch/ + extensions/  (segundos)
  → ssh apply --skip-cargo-build           (segundos)
  
Total: ~30-60s vs 10-20 min compile en Asus.
```

## Error handling

| Situación | Acción |
|---|---|
| Manifest TOML mal formado | log error, **skip esa extensión**, continúa |
| `pacman -S` falla | **abort** (no podemos seguir sin deps) |
| `cargo build` falla | **abort** (no hay binary que instalar) |
| `sudo install` falla | log + **skip esa extensión**, continúa |
| `daemon-reload` falla | log warn, continúa |
| `required_keys` falta en `.env` | warn + **skip enable_on_install**, continúa |
| `verify()` final falla | log warn, exit 0 |

Filosofía: **fail-loud para deps y build** (no hay forma de continuar);
**skip-and-continue para instalación / restart / env** (otras extensiones
pueden seguir funcionando independientes).

Cada skip deja un mensaje claro: `[apply] WARN: extension 'X' saltada por <razón>`.
Resumen final: `applied: N, skipped: M, errors: K`.

## Concurrencia y orden

Las extensiones se aplican **secuencialmente** en orden alfabético por
nombre de carpeta. No hay deps cruzadas entre extensiones en v1; cada una
es independiente. Si en el futuro aparecen deps, se añade
`[meta] depends_on = ["otra"]` y se hace topological sort.

## Testing

v1 sin tests automáticos formales. Justificación: `apply.sh` es bash que
invoca primitivas del sistema (`pacman`, `cargo`, `systemctl`, `sudo
install`); mockear esas primitivas no aporta confianza real comparado con
correr `apply.sh --dry-run` manual.

Lo que sí incluimos en v1:

1. **`apply.sh --dry-run`**: imprime qué haría sin ejecutar nada destructivo.
2. **`apply.sh --check`**: verifica integridad post-apply.
3. **Validación TOML al merge**: `python -c 'import tomllib; tomllib.load(...)'`
   manual antes de commit. Pre-commit hook futuro.

v2 (cuando aparezca dolor real): tests bash con `bats`, sandbox
`/tmp/jarvis-fake-root`.

## Plan de migración

Migración en 4 commits independientes, cada uno revertible y dejando el
sistema funcional.

### M1 — Extraer infraestructura (sin cambios de comportamiento)

- Crear `arch/lib/manifest_get.py`.
- Crear `arch/apply.sh` con todas las funciones implementadas pero **no
  invocado por nadie**. La lógica viva sigue en `install.sh`/`update.sh`
  hardcoded.
- **Validación**: `update.sh` debe dejar el sistema idéntico bit-a-bit.
  Diff sólo añade archivos nuevos.

### M2 — Migrar `jarvis-linux-mcp` a extensión

- Crear `extensions/jarvis-linux-mcp/manifest.toml`.
- Mover `arch/systemd-user/jarvis-mcp-register.service` →
  `extensions/jarvis-linux-mcp/systemd-user/`.
- Quitar de `install.sh`/`update.sh` la lógica hardcoded de jarvis-linux-mcp;
  delegar a `apply.sh`.
- **Validación**: en Asus, `update.sh` deja `ironclaw mcp list` mostrando
  jarvis-linux igual que antes. Las 8 tools listables.

### M3 — Migrar `jarvis-voice-daemon` a extensión

- Igual que M2 pero con voice daemon.
- Mover `JARVIS_VOICE_VARS`, `ELEVENLABS_*` desde `arch/templates/ironclaw.env`
  a `extensions/jarvis-voice-daemon/env/voice.env` con `required_keys`.
- **Validación**: `jarvis-voice-daemon` arranca igual; conversación
  sigue funcionando.

### M4 — Cleanup final

- `install.sh` se reduce a ~30 LoC: deps base + paru + `exec apply.sh`.
- `update.sh` se reduce a `exec apply.sh`.
- `arch/templates/ironclaw.env` solo trae las vars del core (DATABASE_BACKEND,
  ANTHROPIC_API_KEY, AGENT_NAME).

Tras M4, añadir una nueva extensión toca **solo `extensions/<name>/`**:
crear carpeta, escribir manifest, optional assets, commit. Cero ediciones
en bash de install/update.

## Decisiones cerradas (no re-debatir)

| Decisión | Razón |
|---|---|
| Crates Rust quedan en `crates/` | Cero churn; "extensión" es unidad lógica, no física |
| `ironclaw` queda como core | Es el host del agente; las extensiones son lo que extiende |
| Motor en bash + Python tomllib | YAGNI vs binary Rust; cero deps nuevas |
| Manifest format C (modular) | `required_keys` resuelve dolor real (env vars perdidas); flags por unit dan control fino |
| `apply.sh` aplica secuencial alfabético | Deps cruzadas no necesarias en v1; YAGNI |
| Deploy remoto via rsync + ssh | Más simple que cross-compilation toolchain o GitHub Actions |
| Tests sólo `--dry-run` y `--check` en v1 | Sandbox formal es over-engineering hoy |

## Cuestiones abiertas (para writing-plans)

- **Naming de carpetas**: `extensions/jarvis-linux-mcp/` o
  `extensions/linux_mcp/`. Probable: usar el nombre del binary
  (`jarvis-linux-mcp`) por consistencia con `pactl`/`systemctl` listings.
- **Cuándo migra `arch/configs/pipewire/echo-cancel.conf`** a una extensión.
  Es config global del sistema, no específico de un componente. Probable:
  queda como core en v1, evaluamos en v2 si tiene sentido como extensión
  "audio-aec".
- **Wallpaper**: ¿core o extensión? Hoy va con install.sh. Como es asset
  global (no específico de un binary), probable queda en core.
- **Hyprland overrides** (general.conf, keybinds.conf): hoy es lógica
  ad-hoc en update.sh con append idempotente. Fuera de scope v1; cuando
  aparezca el segundo override que los justifique, se evalúa migración.

Estos no bloquean el plan; se resuelven en el momento de escribir el
código de cada paso M1-M4.
