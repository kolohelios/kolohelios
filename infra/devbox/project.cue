package project

#Project & {
	name: "devbox"
	kind: "infra"
	infra: {
		description: "kolohelios — devbox NixOS configuration and Linode image"
		extraInputs: {
			"home-env": {
				url: "https://flakehub.com/f/kolohelios/home/*.tar.gz"
				follows: {
					"kolohelios-nix": "kolohelios-nix"
					"nixpkgs":        "nixpkgs"
				}
			}
		}
		devShellPackages: ["opentofu", "linode-cli"]
		letExtra: """
      lib = nixpkgs.lib;

      # The devbox is always built for x86_64-linux, regardless of the host
      # invoking this flake.
      devboxConfig = lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          ./nixos/configuration.nix
          home-env.nixosModules.home
        ];
      };

      imageConfig = lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          ./nixos/image.nix
          home-env.nixosModules.home
        ];
      };

      x86Pkgs = import nixpkgs {
        system = "x86_64-linux";
        config.allowUnfree = true;
      };
"""
		extra: """
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
"""
	}
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "devbox"
			purpose: "Terraform remote state for the devbox Linode infrastructure"
		},
	]
	ci: {
		build: {
			// Historic filter key + job id — predates the project rename
			// from "image" to "devbox". Kept explicit rather than auto-
			// derived from the project name.
			filterKey:   "image"
			jobId:       "image"
			displayName: "Linode image"
			attr:        "linodeImage"
			publish: {
				kind:             "artifact"
				name:             "linode-image"
				path:             "result/nixos.img"
				retentionDays:    7
				compressionLevel: 6
			}
		}
	}
}
