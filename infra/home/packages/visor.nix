{
  lib,
  stdenvNoCC,
  fetchurl,
  testers,
}:

let
  version = "0.13.3";
  darwin = {
    platform = "darwin_all";
    sha256 = "7138b4940e9d81c27a1330dece2954dbdab241952571cb8f7149bdfdcfd56ac0";
  };
  sources = {
    x86_64-linux = {
      platform = "linux_amd64";
      sha256 = "a2f40b0af0e2991df9c88e529adf9f1c134212c5dd67c4a66bcfe78198025c47";
    };
    aarch64-linux = {
      platform = "linux_arm64";
      sha256 = "e58b94796fa0c7f6811247f1db2a332dc672faf6c975015451fed5cb7da38f98";
    };
    x86_64-darwin = darwin;
    aarch64-darwin = darwin;
  };
  source = sources.${stdenvNoCC.hostPlatform.system};
in
stdenvNoCC.mkDerivation (finalAttrs: {
  pname = "visor";
  inherit version;

  # Upstream release archives include both the CLI and its weather plugin.
  src = fetchurl {
    url = "https://github.com/oronbz/visor/releases/download/v${version}/visor_${version}_${source.platform}.tar.gz";
    inherit (source) sha256;
  };

  sourceRoot = ".";
  dontBuild = true;
  dontStrip = true;

  installPhase = ''
    runHook preInstall
    install -Dm755 visor "$out/bin/visor"
    install -Dm755 visor-weather "$out/bin/visor-weather"
    install -Dm644 LICENSE "$out/share/licenses/visor/LICENSE"
    runHook postInstall
  '';

  passthru.tests.version = testers.testVersion { package = finalAttrs.finalPackage; };

  meta = {
    description = "Heads-up display for your terminal";
    homepage = "https://github.com/oronbz/visor";
    license = lib.licenses.mit;
    platforms = builtins.attrNames sources;
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
    mainProgram = "visor";
  };
})
