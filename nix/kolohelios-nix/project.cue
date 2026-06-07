package project

#Project & {
	name: "kolohelios-nix"
	kind: "nix-lib"
	nixLib: {
		description: "kolohelios — shared nix utilities for project flakes"
		extra: """
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forEachSupportedSystem =
        f:
        nixpkgs.lib.genAttrs supportedSystems (
          system:
          f {
            inherit system;
            pkgs = import nixpkgs {
              inherit system;
              config.allowUnfree = true;
            };
          }
        );

      # Tiny `shaka` on PATH that walks up from cwd looking for the
      # canonical wrapper at `tools/shaka/bin/shaka` and exec's it.
      # Without this, the documented invocation only works from the repo
      # root — agents in a subdirectory or jj workspace hit `no such file
      # or directory`. Walking up (rather than asking git/jj) handles jj
      # workspaces uniformly, since those have no colocated `.git`.
      #
      # In an external repo with no kolohelios checkout above cwd, fall
      # back to a real `shaka` elsewhere on PATH (a FlakeHub-installed
      # binary) instead of erroring — otherwise this shim shadows it and
      # `shaka` is unusable. The shim is itself named `shaka`, so the
      # fallback skips its own store path to avoid exec'ing into itself.
      shakaShim =
        pkgs:
        pkgs.writeShellApplication {
          name = "shaka";
          text = ''
            dir="$PWD"
            while [[ "$dir" != "/" ]]; do
              wrapper="$dir/tools/shaka/bin/shaka"
              if [[ -x "$wrapper" ]]; then
                exec "$wrapper" "$@"
              fi
              dir="$(dirname "$dir")"
            done
            # No kolohelios checkout above cwd. Find the next `shaka` on
            # PATH that isn't this shim and hand off to it. `self` is the
            # absolute path of this script as invoked; resolve it so a
            # symlinked PATH entry still matches the store realpath.
            self="''${BASH_SOURCE[0]}"
            self="$(readlink -f "$self" 2>/dev/null || echo "$self")"
            IFS=':' read -ra path_dirs <<<"$PATH"
            for d in "''${path_dirs[@]}"; do
              cand="$d/shaka"
              [[ -x "$cand" ]] || continue
              real="$(readlink -f "$cand" 2>/dev/null || echo "$cand")"
              [[ "$real" == "$self" ]] && continue
              exec "$cand" "$@"
            done
            echo "shaka: no tools/shaka/bin/shaka found above $PWD" >&2
            echo "and no other shaka on PATH to fall back to" >&2
            exit 1
          '';
        };

      # Workflow tools every project's devShell wants. Consumers compose
      # their own packages by spreading this list and adding project-specific
      # tools on top.
      workflowPackages =
        pkgs:
        (with pkgs; [
          jujutsu
          git
          just
          jq
          cue
          nixfmt-rfc-style
          nil
          typos
          taplo
          cargo-deny
          cargo-machete
          vale
          _1password-cli
        ])
        ++ [ (shakaShim pkgs) ];
    in
    {
      lib = {
        inherit
          supportedSystems
          forEachSupportedSystem
          workflowPackages
          shakaShim
          ;
      };

      formatter = forEachSupportedSystem ({ pkgs, ... }: pkgs.nixfmt-rfc-style);

      # Devshell for working on this project itself (editing the lib's nix
      # files). Same workflow tools every consumer uses.
      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages = workflowPackages pkgs;
          };
        }
      );
    };
"""
	}
	ci: {
		build: {
			filterKey:   "nix-lib"
			jobId:       "nix-lib"
			displayName: "kolohelios-nix"
			// `nix flake check` rather than `nix build` — this is a lib-only
			// flake (lib + formatter + devShells, no derivation output), so
			// the goal is to force eval of every output that flakehub-push
			// then uploads as flake source.
			nixCommand: "check"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/kolohelios-nix"
				visibility: "public"
				rolling:    true
			}
			// Self-notify on a fresh publish so bump-kolohelios-nix runs
			// immediately rather than waiting for its daily cron. This
			// closes the propagation chain: the nixpkgs bump merges here,
			// build-nix-lib republishes kolohelios-nix, and consumers pick
			// up the new nixpkgs revision within minutes instead of ≤24h.
			dispatch: [
				{
					repo:      "kolohelios/kolohelios"
					eventType: "kolohelios-nix-published"
				},
			]
		}
	}
}
