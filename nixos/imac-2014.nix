# nixos/imac-2014.nix
#
# Módulo NixOS específico para el iMac 27" 5K Retina late 2014:
#   - CPU: Intel Core i7-4790K (Haswell-DT, 4 cores / 8 threads, 4.0/4.4 GHz)
#   - GPU: AMD Radeon R9 M295X (Tonga, GCN 1.2, 4 GB GDDR5)
#   - WiFi: Broadcom BCM4360 (necesita driver propietario "wl")
#   - Audio: Cirrus Logic CS4208 (vía snd_hda_intel)
#   - 32 GB DDR3-1600
#
# Este módulo NO presupone instalación en disco — está pensado para una
# ISO live que arranca desde USB con macOS intacto en el disco interno.
{ config, lib, pkgs, ... }:
{
  ##################
  # Boot / kernel  #
  ##################

  # Kernel reciente: mejor soporte de hardware Apple/AMD que el LTS por defecto.
  # `linuxPackages_latest` sigue la rama "latest" de nixpkgs; si en algún
  # momento un kernel rompe algo, podemos pinear a un major concreto.
  boot.kernelPackages = pkgs.linuxPackages_latest;

  # Módulos a cargar al arranque (kernel modules):
  #   kvm-intel  — virtualización Haswell-DT (no esencial pero útil).
  #   applesmc   — SMC de Apple: lectura de sensores (temp, ventiladores).
  #   coretemp   — sensores térmicos del Haswell.
  boot.kernelModules = [ "kvm-intel" "applesmc" "coretemp" ];

  # Driver propietario Broadcom para BCM4360.
  # b43 (open source) no soporta este chip; toca usar broadcom_sta (alias "wl").
  # `extraModulePackages` los compila como out-of-tree contra el kernel actual.
  boot.extraModulePackages = with config.boot.kernelPackages; [ broadcom_sta ];

  # Módulos open source que entran en conflicto con wl: blacklistearlos para
  # que wl tome control exclusivo de la radio Broadcom.
  boot.blacklistedKernelModules = [ "bcma" "b43" "ssb" ];

  # Permitir firmware redistribuible (Intel microcode, BT, etc.).
  # Sin esto, NixOS rehúsa instalar binarios firmware no-libres.
  hardware.enableRedistributableFirmware = true;

  # broadcom_sta es `unfreeRedistributable` Y está marcado `insecure` (el blob
  # no se actualiza desde hace años, tiene CVEs reportadas). Para una live ISO
  # de pruebas en casa es aceptable; en cualquier despliegue serio habría que
  # repensar (USB WiFi externo con chip soportado por kernel libre, p.ej.).
  # Permitimos PUNTUALMENTE (solo este paquete) en vez de abrir las flags
  # globales `allowUnfree`/`allowInsecure`.
  nixpkgs.config.allowUnfreePredicate = pkg:
    builtins.elem (lib.getName pkg) [
      "broadcom-sta"
      # cloudflare-warp arrastrado por illogical-flake (probablemente para
      # widget de VPN status). No lo activamos como servicio, solo permitimos
      # que el paquete exista en el closure.
      "cloudflare-warp"
    ];
  nixpkgs.config.permittedInsecurePackages = [
    "broadcom-sta-6.30.223.271-59-7.0.1"
  ];

  # Microcode Intel — fixes de errata del Haswell, recomendado siempre.
  hardware.cpu.intel.updateMicrocode =
    lib.mkDefault config.hardware.enableRedistributableFirmware;

  ##################
  # GPU            #
  ##################

  # Habilita Mesa, Vulkan loader, OpenCL, etc. Necesario para Wayland/Hyprland.
  hardware.graphics.enable = true;
  hardware.graphics.enable32Bit = true;  # apps 32-bit (Steam etc.) si hicieran falta

  ##################
  # Teclado Apple  #
  ##################

  # hid_apple: ajusta comportamiento de las teclas especiales del teclado Apple.
  #   iso_layout=0  — layout US/ANSI (cambiar a 1 si el teclado fuera ISO/EU).
  #   fnmode=2      — F-keys directas; con fn pulsado, multimedia.
  boot.extraModprobeConfig = ''
    options hid_apple iso_layout=0 fnmode=2
  '';

  ##################
  # Networking     #
  ##################

  # NetworkManager para WiFi: GUI/CLI manejable, evita pelearse con wpa_supplicant.
  networking.networkmanager.enable = true;

  ##################
  # Bluetooth      #
  ##################

  # BCM4360 es chip combo WiFi+BT en el iMac 2014. La parte BT necesita
  # su propio stack (bluez) y firmware (incluido en linux-firmware vía
  # enableRedistributableFirmware arriba).
  hardware.bluetooth = {
    enable = true;
    powerOnBoot = true;        # encender controller al arranque
    settings = {
      General = {
        # Permitir que los teclados/ratones BT se reconecten automáticamente
        # tras el primer pareo (sin tener que volver a parear).
        AutoEnable = true;
        # Apple Magic Keyboard / Trackpad usan perfil HID.
        ControllerMode = "dual";
        # JustWorksRepairing=always: si un dispositivo intenta re-emparejar
        # sin confirmación, lo aceptamos. Útil en live ISO de pruebas porque
        # macOS deja en el teclado el pairing del iMac, y al arrancar Linux
        # querrá reusarlo. Trade-off: más inseguro en redes hostiles.
        JustWorksRepairing = "always";
      };
    };
  };

  # NOTA v9: blueman-applet desactivado tras descubrir en journal de v8
  # que el unit `blueman-applet.service` reporta "bad unit file setting".
  # No es necesario: el daemon `bluetooth.service` (de hardware.bluetooth)
  # gestiona el pareo y conexión sin GUI; `bluetoothctl` desde foot suple
  # cualquier configuración manual. Reactivar si en F2+ se quiere icono BT
  # en el system tray del HUD.
  services.blueman.enable = false;

  ##################
  # Audio          #
  ##################

  # PipeWire (estándar moderno) reemplaza PulseAudio + JACK.
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    alsa.support32Bit = true;
    pulse.enable = true;  # apps que aún hablan PulseAudio
  };
  # rtkit: prioridades realtime para audio sin glitches.
  security.rtkit.enable = true;

  ##################
  # Locale / time  #
  ##################

  time.timeZone = lib.mkDefault "Europe/Madrid";
  i18n.defaultLocale = lib.mkDefault "es_ES.UTF-8";
}
