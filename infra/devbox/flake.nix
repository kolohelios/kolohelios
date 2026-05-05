{
  description = "kolohelios — devbox NixOS configuration and Linode image";

  inputs = {
    kolohelios-nix.url = "https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz";
    nixpkgs.follows = "kolohelios-nix/nixpkgs";
  };

  outputs =
    {
      self,
      kolohelios-nix,
      nixpkgs,
      ...
    }:
    let
      inherit (kolohelios-nix.lib) forEachSupportedSystem workflowPackages;
      lib = nixpkgs.lib;

      # The devbox is always built for x86_64-linux, regardless of the host
      # invoking this flake.
      devboxConfig = lib.nixosSystem {
        system = "x86_64-linux";
        modules = [ ./nixos/configuration.nix ];
      };

      imageConfig = lib.nixosSystem {
        system = "x86_64-linux";
        modules = [ ./nixos/image.nix ];
      };

      x86Pkgs = import nixpkgs {
        system = "x86_64-linux";
        config.allowUnfree = true;
      };
    in
    {
      nixosConfigurations.devbox = devboxConfig;

      packages.x86_64-linux.linodeImage = imageConfig.config.system.build.linodeImage;

      # Eval check is x86_64-linux-only by design: NixOS configs are
      # evaluated for the deploy target. Hosts on other systems (e.g.
      # aarch64-darwin) get an empty `checks.<host>` set, which `nix flake
      # check` skips cleanly.
      checks.x86_64-linux.devbox-eval = x86Pkgs.runCommand "devbox-eval-check" { } ''
        : "${devboxConfig.config.system.build.toplevel.drvPath}"
        : "${imageConfig.config.system.build.toplevel.drvPath}"
        echo "nixosConfigurations.devbox + linodeImage evaluate" > $out
      '';

      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages =
              (workflowPackages pkgs)
              ++ (with pkgs; [
                opentofu
                linode-cli
              ]);
          };
        }
      );

      formatter = kolohelios-nix.formatter;
    };
}
