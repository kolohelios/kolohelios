package project

#Project & {
	name: "home"
	kind: "infra"
	infra: {
		description: "kolohelios — cross-platform home environment"
		extraInputs: {
			"home-manager": {
				url: "github:nix-community/home-manager/release-26.05"
				follows: {
					"nixpkgs": "nixpkgs"
				}
			}
			"nix-darwin": {
				url: "github:LnL7/nix-darwin/nix-darwin-26.05"
				follows: {
					"nixpkgs": "nixpkgs"
				}
			}
			"claude-hooks": {
				url: "https://flakehub.com/f/kolohelios/claude-hooks/*.tar.gz"
				follows: {
					"kolohelios-nix": "kolohelios-nix"
					"nixpkgs":        "nixpkgs"
				}
			}
			// The real `shaka` CLI, consumed from FlakeHub so it can be
			// put on PATH globally (modules/common.nix). Without it, the
			// shim-only install resolved `shaka` only inside a kolohelios
			// checkout; `/start` / `/ship` failed in external repos.
			"shaka": {
				url: "https://flakehub.com/f/kolohelios/shaka/*.tar.gz"
				follows: {
					"kolohelios-nix": "kolohelios-nix"
					"nixpkgs":        "nixpkgs"
				}
			}
		}
		extra: """
      darwinConfigurations.Jons-MacBook-Pro = nix-darwin.lib.darwinSystem {
        system = "aarch64-darwin";
        specialArgs = { inherit claude-hooks kolohelios-nix shaka; };
        modules = [
          home-manager.darwinModules.home-manager
          ./modules/darwin.nix
        ];
      };

      # NixOS module — imported by `infra/devbox` to apply this user's
      # home-manager profile to the `jon` account on the devbox.
      # `_module.args` is the NixOS equivalent of the `specialArgs`
      # plumbing above (claude-hooks, kolohelios-nix) so `infra/devbox`
      # doesn't have to know about them.
      nixosModules.home = {
        imports = [
          home-manager.nixosModules.home-manager
          ./modules/linux.nix
        ];
        _module.args = { inherit claude-hooks kolohelios-nix shaka; };
      };
"""
	}
	ci: {
		build: {
			filterKey:   "home"
			jobId:       "home"
			displayName: "infra/home"
			// Lib-style flake (nixosModules + darwinConfigurations + devShells,
			// no native build output); `nix flake check` forces eval of every
			// output that flakehub-push then uploads as source.
			nixCommand: "check"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/home"
				visibility: "public"
				rolling:    true
			}
		}
	}
}
