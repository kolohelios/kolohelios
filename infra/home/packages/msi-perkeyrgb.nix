{
  lib,
  fetchzip,
  python3Packages,
  hidapi,
  usbutils,
}:

python3Packages.buildPythonApplication {
  pname = "msi-perkeyrgb";
  version = "2.1-unstable";
  pyproject = true;

  src = fetchzip {
    extension = "tar.gz";
    url = "https://codeload.github.com/Askannz/msi-perkeyrgb/tar.gz/e185a29e864bdda952b336940b047b5f97419d46";
    sha256 = "0f25png4fcf7n07g57aa8nc2z3524ydx41b1vzh4dyij39r8lvs0";
  };

  build-system = [ python3Packages.setuptools ];

  postPatch = ''
    # Nix has no global ldconfig cache; use the pinned hidraw backend.
    substituteInPlace msi_perkeyrgb/hidapi_wrapping.py \
      --replace-fail 's = popen("ldconfig -p").read()' \
        's = "${lib.getLib hidapi}/lib/libhidapi-hidraw.so.0"' \
      --replace-fail 'popen("lsusb")' 'popen("${usbutils}/bin/lsusb")'
    # Match HIDAPI's size_t ABI on 64-bit hosts.
    substituteInPlace msi_perkeyrgb/hidapi_types.py \
      --replace-fail 'hid_send_feature_report.argtypes = [ct.c_void_p, ct.c_char_p, ct.c_int]' \
        'hid_send_feature_report.argtypes = [ct.c_void_p, ct.c_char_p, ct.c_size_t]'
  '';

  # NixOS consumers can use services.udev.packages = [ thisPackage ].
  # Only grant the active local user access to this keyboard controller.
  postInstall = ''
    mkdir -p "$out/lib/udev/rules.d"
    echo 'SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1038", ATTRS{idProduct}=="113a", TAG+="uaccess"' \
      > "$out/lib/udev/rules.d/60-msi-perkeyrgb.rules"
  '';

  pythonImportsCheck = [ "msi_perkeyrgb.main" ];

  meta = {
    description = "Control SteelSeries per-key RGB backlighting on MSI laptops";
    homepage = "https://github.com/Askannz/msi-perkeyrgb";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "msi-perkeyrgb";
  };
}
