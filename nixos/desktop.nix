# nixos/desktop.nix
#
# Configuración de escritorio para jarvis-os:
#   - Hyprland (wlroots-based Wayland compositor con layer-shell nativo,
#     necesario para el HUD envolvente que vendrá en F2+).
#   - greetd como display manager con autologin al usuario "jarvis".
#   - Paquetes Wayland básicos para una sesión usable desde el primer boot.
{ config, lib, pkgs, jarvisVersion, ... }:
{
  ##################
  # Compositor     #
  ##################

  # programs.hyprland.enable: configura Hyprland a nivel sistema (PAM, env vars,
  # XDG portals, polkit). Equivalente a "hyprland está disponible y se integra
  # con NixOS". El lanzamiento real lo hace UWSM (más abajo).
  programs.hyprland = {
    enable = true;
    xwayland.enable = true;  # apps X11 vía Xwayland (algunas apps aún no Wayland)
    # Integra Hyprland con UWSM (Universal Wayland Session Manager) — patrón
    # recomendado por upstream Hyprland desde 0.42. Sin esto, Hyprland imprime
    # el warning "started without start-hyprland" y no obtenemos integración
    # con systemd-user (que necesitaremos para voice_daemon como user-service).
    withUWSM = true;
  };

  ##################
  # Display mgr    #
  ##################

  # greetd: display manager minimalista basado en greetd-protocol.
  # initial_session: arranque automático del usuario "jarvis" → Hyprland.
  # Como nuestra ISO es stateless (live USB), CADA boot es "primer boot",
  # así que initial_session se aplica siempre.
  #
  # Comando: `uwsm start hyprland-uwsm.desktop` lanza Hyprland bajo systemd-user,
  # con env vars D-Bus/XDG correctamente expuestas a hijos del proceso.
  services.greetd = {
    enable = true;
    settings = {
      default_session = {
        # Si initial_session falla, mostramos tuigreet (TUI) para login manual.
        # Ahí también lanzamos via UWSM para coherencia.
        command = "${pkgs.greetd.tuigreet}/bin/tuigreet --time --remember --cmd 'uwsm start hyprland-uwsm.desktop'";
        user = "greeter";
      };
      initial_session = {
        command = "uwsm start hyprland-uwsm.desktop";
        user = "jarvis";
      };
    };
  };

  ##################
  # Usuario        #
  ##################

  users.users.jarvis = {
    isNormalUser = true;
    description = "jarvis-os primary user";
    extraGroups = [
      "wheel"          # sudo
      "networkmanager" # gestionar WiFi sin sudo
      "video"          # acceso a GPU
      "audio"          # acceso a dispositivos audio
      "input"          # leer eventos teclado/mouse (para futuro voice trigger)
    ];
    # Solo para live ISO de pruebas. Si esto se llevara a algo persistente,
    # usar `hashedPassword` y NO meterlo en git.
    initialPassword = "jarvis";
  };

  # En live ISO permitimos sudo sin password (es una máquina de pruebas).
  # En modo persistente esto se revisa.
  security.sudo.wheelNeedsPassword = false;

  ##################
  # Paquetes base  #
  ##################

  environment.systemPackages = with pkgs; [
    # Utilidades básicas
    vim git curl wget htop file tree

    # Terminal Wayland-nativo (más rápido y ligero que alacritty/kitty en Wayland)
    foot

    # ─── v16 UI primary: end-4 dots-hyprland (Material ii) ───
    # El home-manager module `programs.dots-hyprland` (declarado en flake.nix
    # nixosConfigurations) instala el shell + sus dependencias en la sesión
    # del usuario `jarvis`. NO añadimos paquetes globales aquí — vive a nivel
    # user. El shell Quickshell incluye su propio status bar, launcher,
    # notif daemon, lock screen, control center, sidebars con AI chat
    # (que en v17 cablearemos a IronClaw).
    fuzzel  # Launcher que end-4 espera por defecto.

    # Capturas y portapapeles Wayland
    grim
    slurp
    wl-clipboard

    # Wallpaper daemon Wayland (carga la imagen y la mantiene como fondo).
    hyprpaper

    # Útiles para debug de hardware en la ISO
    pciutils    # lspci
    usbutils    # lsusb
    lshw
    iw          # info WiFi

    # Diagnóstico Wayland / input — útiles cuando teclado o input falla
    wev         # ver eventos Wayland (qué keysym/modifier produce cada tecla)
    evtest      # eventos evdev raw del kernel (sin pasar por Wayland)

    # Editores ligeros para tocar config in place sin reflashear
    helix       # modal moderno
    nano        # baseline universal

    # Tooling para shell scripts del live
    jq
    ripgrep
    bind        # dig, host
    inetutils   # telnet, traceroute, ftp

    # ─── Pulido visual v10 ───
    # Tema GTK (Adwaita-dark viene built-in, pero gnome-themes-extra
    # lo expone como tema seleccionable y trae variantes completas).
    gnome-themes-extra
    adwaita-icon-theme

    # Cursor con tamaño decente para HiDPI 5K (default X cursor es 16px,
    # ridículo en pantalla 5K). Bibata es minimal + bien rasterizado.
    bibata-cursors

    # Necesario para que apps Qt (si aparece alguna) no se vean rotas.
    qt5.qtwayland
    qt6.qtwayland

    # gsettings + dconf para que cambios de tema persistan en sesión.
    glib
    gsettings-desktop-schemas
  ];

  # XDG portals — necesarios para que apps Wayland (file pickers, screensharing)
  # funcionen bien.
  xdg.portal = {
    enable = true;
    extraPortals = [ pkgs.xdg-desktop-portal-hyprland ];
  };

  ##################
  # Deps shell HUD #
  ##################

  # Servicios requeridos por illogical-impulse (end-4 dots-hyprland) en NixOS.
  # Documentados en https://github.com/soymou/illogical-flake README:
  #   - upower: estado batería/AC para widgets de sistema.
  #   - geoclue2: posicionamiento (QtPositioning, usado por widgets de hora/clima).
  #   - power-profiles-daemon: profiles ahorro/balanceado/performance.
  services.upower.enable = true;
  services.geoclue2.enable = true;
  services.power-profiles-daemon.enable = true;

  # Fuentes para que el escritorio se vea bien en HiDPI Retina del iMac 5K.
  fonts = {
    packages = with pkgs; [
      # Sans serif moderno, perfecto para UI (waybar, wofi, foot prompts).
      inter

      # Cobertura amplia de scripts (cyrílico, asiáticos, etc.).
      noto-fonts
      noto-fonts-cjk-sans
      noto-fonts-color-emoji

      # Mono con ligaturas para terminales y código.
      fira-code
      fira-code-symbols

      # Nerd Font para iconos en waybar/wofi (Powerline, Material, etc.).
      nerd-fonts.fira-code

      # ─── v17 fuentes requeridas por illogical-impulse ───
      # https://github.com/soymou/illogical-flake README.
      rubik
      nerd-fonts.ubuntu
      nerd-fonts.jetbrains-mono
    ];

    # fontconfig: define fuentes por defecto + parámetros de rendering.
    # En HiDPI Retina el rendering es crítico — sin esto las letras se ven
    # "blandas" porque libfreetype usa defaults conservadores.
    fontconfig = {
      defaultFonts = {
        sansSerif = [ "Inter" "Noto Sans" ];
        serif = [ "Noto Serif" ];
        monospace = [ "Fira Code" "Noto Sans Mono" ];
        emoji = [ "Noto Color Emoji" ];
      };
      # subpixel rendering: las pantallas LCD tienen 3 subpixels (RGB) por
      # pixel; aprovecharlos da nitidez extra. iMac 5K Retina es RGB stripe.
      subpixel = {
        rgba = "rgb";
        lcdfilter = "default";
      };
      # Hinting: ajusta letras al grid de pixel. "slight" da el balance
      # entre fidelidad al diseño y nitidez en HiDPI.
      hinting = {
        enable = true;
        style = "slight";
      };
      antialias = true;
    };
  };

}
