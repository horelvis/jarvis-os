{
  # description: aparece en `nix flake show` y en FlakeHub si se publicara.
  description = "jarvis-os: Linux-integrated agentic OS (evolution of IronClaw)";

  # inputs: dependencias externas del flake. Cada entrada es OTRO flake (o un
  # tarball/repo equivalente). Nix las descarga, evalúa, y deja sus outputs
  # disponibles para usar abajo. Las versiones exactas se fijan en flake.lock
  # la primera vez que se evalúa, garantizando reproducibilidad.
  inputs = {
    # nixpkgs: la colección masiva (~80k) de paquetes mantenida por NixOS.
    # Apuntamos a la rama nixos-unstable para tener versiones recientes de
    # todo (Rust, libs, herramientas). Si quisiéramos estabilidad, usaríamos
    # "nixos-25.11" (release stable actual de NixOS).
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    # rust-overlay: provee toolchains de Rust de cualquier versión exacta
    # (1.92.0, beta, nightly-2026-04-15, etc.). nixpkgs solo trae la versión
    # del canal; rust-overlay nos deja pinear lo que diga Cargo.toml.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      # follows: en lugar de que rust-overlay descargue otro nixpkgs propio,
      # le decimos "usa el mismo nixpkgs de arriba". Menos duplicación,
      # menos descarga, builds más rápidas.
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # ─── Quickshell desktop shell (v17/v18) ───
    # end-4/dots-hyprland (Material ii / illogical-impulse) vía soymou.
    # IronClaw diagnosticó (2026-04-29) la causa real del silent fail v17:
    # nuestro `system.activationScripts.jarvisHyprlandConfig` pre-creaba
    # /home/jarvis/.config/hypr/hyprland.conf como root antes de que
    # home-manager activara, y home-manager rehúsa clobberar.
    # v18 elimina nuestro activation script de hyprland.conf — illogical
    # owns hyprland.conf completo via home-manager. Re-añadiremos custom
    # binds nuestros (F1.5 Cmd+Y/N) en v19+ via home-manager xdg.configFile.
    illogical-flake = {
      url = "github:soymou/illogical-flake";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  # outputs: lo que el flake "exporta". El argumento es un attrset con todos
  # los inputs ya evaluados.
  #
  # Dejamos de usar flake-utils porque mezcla mal con `nixosConfigurations`
  # (que NO es per-system: una nixosConfiguration ya tiene su system pinneado
  # dentro). Hacemos forAllSystems a mano: más explícito y cero magia.
  outputs = { self, nixpkgs, rust-overlay, illogical-flake, home-manager, ... }:
    let
      # Versión de jarvis-os. Incrementar manualmente al rebuild para que
      # se distinga la ISO en el iMac (ver /etc/jarvis-version y nombre del .iso).
      # Convención: vMAJOR.MINOR.PATCH donde MINOR avanza por iteración funcional
      # de la live ISO, PATCH por hotfix de config.
      jarvisVersion = "0.19.0";

      # Sistemas para los que generamos devShells y packages.
      # x86_64-linux cubre el dev box (i9) y el target (iMac 2014).
      systems = [ "x86_64-linux" ];

      # Helper: ejecuta `f system` para cada system y devuelve attrset
      # { x86_64-linux = ...; aarch64-linux = ...; ... }.
      forAllSystems = nixpkgs.lib.genAttrs systems;

      # pkgs por sistema, con rust-overlay aplicado.
      pkgsFor = system: import nixpkgs {
        inherit system;
        overlays = [ (import rust-overlay) ];
      };

      ###################################################################
      # Paquetes Rust de jarvis-os.
      ###################################################################
      # Construye los binarios de las crates jarvis_* via
      # rustPlatform.buildRustPackage. Usamos el Cargo.lock raíz del
      # workspace para resolver las versiones; cargoBuildFlags filtra
      # qué paquete/binario se compila para no construir IronClaw entero.
      jarvisRustPackages = system:
        let
          pkgs = pkgsFor system;

          # Toolchain Rust pinneado igual que el devShell (1.92.0).
          rust = pkgs.rust-bin.stable."1.92.0".default;

          # Argumentos comunes a todos los paquetes Rust del workspace.
          # Reduce duplicación entre las derivations.
          commonRustArgs = {
            version = jarvisVersion;
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
              allowBuiltinFetchGit = true;  # deps git de IronClaw
            };
            cargoBuildType = "release";
            cargo = rust;
            rustc = rust;
            doCheck = false;
            nativeBuildInputs = with pkgs; [ pkg-config cmake ];
          };
        in {
          jarvis-linux-mcp = pkgs.rustPlatform.buildRustPackage (commonRustArgs // {
            pname = "jarvis-linux-mcp";
            cargoBuildFlags = [ "-p" "jarvis_linux_mcp" "--bin" "jarvis-linux-mcp" ];
            meta = {
              description = "Linux MCP server for jarvis-os: systemd, polkit, D-Bus tools";
              homepage = "https://github.com/horelvis/jarvis-os";
              license = with pkgs.lib.licenses; [ mit asl20 ];
            };
          });

          # IronClaw root binary — el "núcleo agéntico" reescrito en Rust.
          # Build pesado (~711 crates), tarda 5-15 min la primera vez.
          # Usamos features default: postgres + libsql + html-to-markdown + tui.
          # Para una live ISO mínima podríamos tirar a `--no-default-features
          # --features libsql,tui` pero la diferencia de tamaño es marginal y
          # mantener defaults nos da paridad con dev box.
          ironclaw = pkgs.rustPlatform.buildRustPackage (commonRustArgs // {
            pname = "ironclaw";
            cargoBuildFlags = [ "-p" "ironclaw" "--bin" "ironclaw" ];
            # IronClaw arrastra deps que pueden necesitar más libs nativas
            # en build time (tokenizers, faiss, etc.). Si falla, añadir aquí.
            buildInputs = with pkgs; [ openssl ];

            # Workaround: monty-0.0.16 (dep de pydantic via git) tiene un
            # `#![doc = include_str!("../../../README.md")]` que apunta fuera
            # del crate. Nix vendora solo el crate, no el README, así que
            # falla. Borramos la línea — pierde docstring del README, no
            # afecta a la compilación ni al runtime.
            preBuild = ''
              if [ -f "$NIX_BUILD_TOP/cargo-vendor-dir/monty-0.0.16/src/lib.rs" ]; then
                sed -i '/^#!\[doc = include_str!/d' \
                  "$NIX_BUILD_TOP/cargo-vendor-dir/monty-0.0.16/src/lib.rs"
              fi
            '';

            meta = {
              description = "IronClaw — secure personal AI assistant (Rust core for jarvis-os)";
              homepage = "https://github.com/nearai/ironclaw";
              license = with pkgs.lib.licenses; [ mit asl20 ];
            };
          });
        };
    in
    {
      ###################################################################
      # devShells: entorno de desarrollo para `nix develop`.
      ###################################################################
      devShells = forAllSystems (system:
        let
          pkgs = pkgsFor system;

          # Toolchain Rust 1.92 estable, coincide con `rust-version` de Cargo.toml.
          rustToolchain = pkgs.rust-bin.stable."1.92.0".default.override {
            extensions = [ "rust-src" "rust-analyzer" ];
          };
        in {
          default = pkgs.mkShell {
            name = "jarvis-os-dev";

            packages = with pkgs; [
              rustToolchain  # rustc 1.92 + cargo + rustfmt + clippy + rust-analyzer
              pkg-config
              cmake
              git
              cacert
            ];

            SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";

            shellHook = ''
              echo ""
              echo "  jarvis-os devShell"
              echo "    rustc:   $(rustc --version)"
              echo "    cargo:   $(cargo --version)"
              echo ""
            '';
          };
        });

      ###################################################################
      # nixosConfigurations: definiciones completas de sistema NixOS.
      # Cada entrada es un sistema construible (live ISO, máquina física,
      # VM, container).
      ###################################################################
      nixosConfigurations.imac-2014 = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";

        # `specialArgs` nos deja pasar valores arbitrarios a TODOS los módulos
        # como argumentos. Los módulos los reciben con el patrón
        # `{ jarvisVersion, jarvisPackages, ... }: { ... }`.
        specialArgs = {
          inherit jarvisVersion;
          jarvisPackages = jarvisRustPackages "x86_64-linux";
        };

        # Los módulos componen la configuración. NixOS los fusiona usando
        # un sistema de tipos: cada `option` declarada se merge con
        # política definida (mkDefault, mkForce, mkMerge...).
        modules = [
          # v19: target hardware = ASUS ZenBook UX431FLC (laptop moderno).
          # iMac 2014 quedó como módulo legacy en `./nixos/imac-2014.nix`
          # — referenciable si alguna vez se quiere volver a probar.
          ./nixos/laptop-asus.nix    # hardware Asus UX431FLC (NVIDIA + Intel)
          ./nixos/desktop.nix        # Hyprland + autologin + paquetes
          ./nixos/iso.nix            # empaquetado como live ISO

          # Home-manager NixOS module para configurar programa
          # `programs.dots-hyprland` del flake end-4-flakes a nivel de usuario.
          home-manager.nixosModules.home-manager
          {
            home-manager.useGlobalPkgs = true;
            home-manager.useUserPackages = true;
            home-manager.users.jarvis = { lib, ... }: {
              imports = [ illogical-flake.homeManagerModules.default ];
              # API limpia: solo enable, el módulo gestiona dotfiles + Python +
              # theming + persistencia internamente. Customización viene en v18.
              programs.illogical-impulse.enable = true;
              # home.stateVersion debe coincidir con el sistema (25.05).
              home.stateVersion = "25.05";

              # Wallpaper jarvis-os disponible en el home del usuario.
              # illogical-impulse tiene wallpaper picker en su sidebar — el
              # usuario apunta a este path en el primer boot y matugen
              # deriva paleta Material You desde aquí (cyan + ámbar dark).
              home.file."Pictures/jarvis-os/wallpaper.jpg".source =
                ./assets/wallpaper.jpg;

              # ─── v19 patches a illogical-impulse upstream ───
              # Comentar la línea obsoleta `gesture_distance = 300` en
              # ~/.config/hypr/hyprland/general.conf — Hyprland reciente
              # la parsea como `finger count` y rechaza valores fuera de 2-4,
              # mostrando un banner de error en pantalla.
              # Como home-manager genera el archivo como symlink read-only
              # del Nix store, hay que romperlo a writable copy y sedear.
              # Cuando illogical upstream lo arregle, este bloque se elimina.
              home.activation.jarvisPatchHyprlandGeneralConf =
                lib.hm.dag.entryAfter [ "writeBoundary" ] ''
                  CONF="$HOME/.config/hypr/hyprland/general.conf"
                  if [ -L "$CONF" ]; then
                    ORIG=$(readlink -f "$CONF")
                    $DRY_RUN_CMD rm "$CONF"
                    $DRY_RUN_CMD cp "$ORIG" "$CONF"
                    $DRY_RUN_CMD chmod u+w "$CONF"
                  fi
                  if [ -f "$CONF" ] && grep -q "gesture_distance = 300" "$CONF"; then
                    $DRY_RUN_CMD sed -i \
                      's|^\([[:space:]]*\)gesture_distance = 300|\1# gesture_distance = 300  # patched by jarvis-os v19: obsolete in modern Hyprland, was triggering finger-count parser error|' \
                      "$CONF"
                  fi
                '';
            };
            # specialArgs también tienen que llegar al home-manager scope.
            home-manager.extraSpecialArgs = { inherit jarvisVersion; };
          }
        ];
      };

      ###################################################################
      # packages.<system>.iso: alias para que `nix build .#iso` funcione.
      # Apunta al output `system.build.isoImage` que iso-image.nix expone.
      ###################################################################
      packages = forAllSystems (system: {
        iso = self.nixosConfigurations.imac-2014.config.system.build.isoImage;
      } // jarvisRustPackages system);
    };
}
