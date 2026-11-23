{
  description = "Nushell agent plugin";

  inputs = {
    base-nixpkgs.url = "github:ck3mp3r/flakes?dir=base-nixpkgs";
    nixpkgs.follows = "base-nixpkgs/unstable";
    # Zola 0.22.x for the docs site. The nixpkgs above carries zola 0.23,
    # which moved to Tera 2 and cannot parse the adidoks theme pinned by
    # docs/themes/adidoks (Tera 1 macros). 0.22.1 is also the version the
    # deploy action runs (shalzz/zola-deploy-action v0.22.1).
    nixpkgs-zola.url = "github:NixOS/nixpkgs/b5553f65a977d833d9e25bcc15fd4f8569ab61da";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    rustnix = {
      url = "github:ck3mp3r/flakes?dir=rustnix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {
    self,
    flake-parts,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux"];
      perSystem = {system, ...}: let
        overlays = [
          inputs.base-nixpkgs.overlays.default
        ];
        pkgs = import inputs.nixpkgs {inherit system overlays;};

        # zola 0.22.x from the pinned nixpkgs-zola input; the nixpkgs above
        # carries 0.23, which the docs theme cannot parse. See flake.nix.
        zola = (import inputs.nixpkgs-zola {inherit system;}).zola;

        # rustnix derives pname from workspace.package.name on virtual
        # manifests; 'name' is not a real inheritable cargo field, so it is
        # synthesized here instead of polluting Cargo.toml.
        cargoToml = let
          raw = fromTOML (builtins.readFile ./Cargo.toml);
        in
          raw
          // {
            workspace =
              raw.workspace
              // {
                package = (raw.workspace.package or {}) // {name = "nu-agent";};
              };
          };
        cargoLock = {lockFile = ./Cargo.lock;};

        # Import packaging logic
        packaging = import ./nix/packaging.nix {
          inherit
            inputs
            system
            pkgs
            cargoToml
            cargoLock
            overlays
            self
            ;
        };
      in {
        inherit (packaging) apps packages;

        devShells = {
          # Regular shell for development
          default = import ./nix/dev.nix {
            inherit inputs pkgs system zola;
          };

          # Classic shell for CI
          ci = import ./nix/ci.nix {
            inherit pkgs inputs system;
          };
        };

        formatter = pkgs.alejandra;
      };

      flake = {
        overlays.default = final: prev: let
          system = final.stdenv.hostPlatform.system;
        in {
          nu-agent-plugin = self.packages.${system}.plugin;
          nu-agent-cli = self.packages.${system}.agent;
          nu-agent = self.packages.${system}.default;
        };
      };
    };
}
