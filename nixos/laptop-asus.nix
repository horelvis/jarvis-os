# nixos/laptop-asus.nix
#
# Módulo NixOS específico para ASUS ZenBook UX431FLC.
#
# Identificado por IronClaw mismo (2026-04-29) vía dmidecode + lspci dentro de
# una sesión live. Hardware:
#   - CPU: Intel Core i7-10510U (Comet Lake-U, 4c/8t, 1.8-4.9 GHz)
#   - GPU iGPU: Intel UHD Graphics CometLake GT2 (PCI 8086:9b41) → i915
#   - GPU dGPU: NVIDIA GeForce MX250 GP108BM (PCI 10de:1d52) → Pascal arch
#   - WiFi: Intel Comet Lake PCH-LP CNVi (PCI 8086:02f0)
#   - BT:   Intel Bluetooth 9460/9560 (USB 8087:0aaa)
#   - Audio: Intel cAVS (PCI 8086:02c8)
#   - 16 GB RAM, NVMe 476 GB Samsung
#   - Display: 1920×1080 eDP (NO HiDPI — sin issues de scaling del iMac 5K).
#
# Comparado con iMac 2014: hardware mucho más amigable a Linux. Todo Intel
# mainline + NVIDIA Pascal con driver proprietary maduro. Sin blobs unfree
# problemáticos (broadcom-sta queda solo en imac-2014.nix).
{ config, lib, pkgs, ... }:
{
  ##################
  # Boot / kernel  #
  ##################

  # Kernel reciente: mejor soporte HW y NVIDIA driver compatibility.
  boot.kernelPackages = pkgs.linuxPackages_latest;

  # KVM Intel para virtualización (VT-x confirmado en CPU).
  boot.kernelModules = [ "kvm-intel" ];

  # Permitir firmware redistribuible — Intel WiFi/BT lo necesitan.
  hardware.enableRedistributableFirmware = true;
  hardware.cpu.intel.updateMicrocode =
    lib.mkDefault config.hardware.enableRedistributableFirmware;

  ##################
  # GPU híbrida    #
  ##################

  # Drivers gráficos: stack moderno hardware.graphics (renombrado desde
  # hardware.opengl). Habilita Mesa, Vulkan, OpenCL.
  hardware.graphics = {
    enable = true;
    enable32Bit = true;  # apps 32-bit (Steam, Wine si hace falta)
    extraPackages = with pkgs; [
      intel-media-driver       # VAAPI Intel iGPU (decode/encode HW)
      libva-vdpau-driver       # bridge VDPAU → VAAPI (renombrado desde vaapiVdpau)
      libvdpau-va-gl
    ];
  };

  # NVIDIA proprietary obligatorio: MX250 es Pascal (GP108), el open kernel
  # module solo soporta Turing (>= RTX 20xx) y posteriores.
  services.xserver.videoDrivers = [ "nvidia" ];
  hardware.nvidia = {
    modesetting.enable = true;     # crítico para Wayland/Hyprland
    open = false;                  # Pascal no soporta open module
    nvidiaSettings = true;         # `nvidia-settings` GUI disponible

    # Driver branch: production (estable). Si dan problemas con kernel
    # latest, alternativas: stable, beta, latest.
    package = config.boot.kernelPackages.nvidiaPackages.production;

    # PRIME: configuración Optimus para combinar iGPU + dGPU.
    # Tres modos: offload (low power, dGPU on demand), sync (siempre dGPU,
    # output via iGPU), reverseSync (dGPU drives display directly).
    # MX250 + display vía iGPU eDP → SYNC para max compatibilidad Wayland.
    # Si quisiéramos batería primero, cambiar a offload.
    prime = {
      sync.enable = true;
      intelBusId  = "PCI:0:2:0";
      nvidiaBusId = "PCI:1:0:0";
    };

    # Power management para suspend/resume estable en laptop.
    powerManagement.enable = true;
  };

  # Permitir paquetes unfree NVIDIA + closure deps de illogical-impulse.
  nixpkgs.config.allowUnfreePredicate = pkg:
    builtins.elem (lib.getName pkg) [
      "nvidia-x11"
      "nvidia-settings"
      "nvidia-persistenced"
      # cloudflare-warp: arrastrado por illogical-flake (closure dep, NO se
      # activa como servicio). Se permite pasivamente.
      "cloudflare-warp"
    ];

  # Env vars críticos para Hyprland sobre NVIDIA Wayland.
  # Sin estos, glitches de cursor/render son casi seguros.
  environment.sessionVariables = {
    LIBVA_DRIVER_NAME = "nvidia";
    GBM_BACKEND = "nvidia-drm";
    __GLX_VENDOR_LIBRARY_NAME = "nvidia";
    # Bug workaround NVIDIA + wlroots: cursor de hardware parpadea/desaparece.
    WLR_NO_HARDWARE_CURSORS = "1";
    # Backend directo nvdec para decode video acelerado.
    NVD_BACKEND = "direct";
  };

  ##################
  # Networking     #
  ##################

  # NetworkManager para WiFi (Intel CNVi soporta out-of-box con linux-firmware).
  networking.networkmanager.enable = true;

  ##################
  # Bluetooth      #
  ##################

  # Intel Bluetooth 9460/9560 — driver bcma + btusb mainline. Sin blobs.
  hardware.bluetooth = {
    enable = true;
    powerOnBoot = true;
    settings = {
      General = {
        AutoEnable = true;
        ControllerMode = "dual";
        # Magic Keyboard / mouse Apple no nos preocupan en Asus, pero
        # "JustWorksRepairing" facilita parear cualquier dispositivo BLE
        # sin prompts excesivos en modo live.
        JustWorksRepairing = "always";
      };
    };
  };

  ##################
  # Audio          #
  ##################

  # Intel cAVS necesita sof-firmware (incluido en linux-firmware moderno) y
  # PipeWire estándar. Stack idéntico al iMac, solo cambia el chipset.
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    alsa.support32Bit = true;
    pulse.enable = true;
  };
  security.rtkit.enable = true;

  ##################
  # Locale / time  #
  ##################

  time.timeZone = lib.mkDefault "Europe/Madrid";
  i18n.defaultLocale = lib.mkDefault "es_ES.UTF-8";

  ##################
  # Power saving   #
  ##################

  # TLP optimiza batería en laptop. Conservador por defecto, no agresivo.
  services.tlp = {
    enable = true;
    settings = {
      CPU_SCALING_GOVERNOR_ON_AC = "performance";
      CPU_SCALING_GOVERNOR_ON_BAT = "powersave";
      START_CHARGE_THRESH_BAT0 = 75;
      STOP_CHARGE_THRESH_BAT0 = 85;
    };
  };
  services.power-profiles-daemon.enable = lib.mkForce false;  # conflicto con TLP
  services.upower.enable = true;
}
