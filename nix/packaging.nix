{
  inputs,
  system,
  pkgs,
  cargoToml,
  cargoLock,
  overlays,
}: let
  supportedTargets = ["aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux"];

  # Install data for pre-built releases (will be generated during release)
  installData = {
    aarch64-darwin =
      if builtins.pathExists ../data/aarch64-darwin.json
      then builtins.fromJSON (builtins.readFile ../data/aarch64-darwin.json)
      else {};
    aarch64-linux =
      if builtins.pathExists ../data/aarch64-linux.json
      then builtins.fromJSON (builtins.readFile ../data/aarch64-linux.json)
      else {};
    x86_64-darwin =
      if builtins.pathExists ../data/x86_64-darwin.json
      then builtins.fromJSON (builtins.readFile ../data/x86_64-darwin.json)
      else {};
    x86_64-linux =
      if builtins.pathExists ../data/x86_64-linux.json
      then builtins.fromJSON (builtins.readFile ../data/x86_64-linux.json)
      else {};
  };

  # Build the entire workspace in one derivation. No `-p` flag means cargo
  # builds every workspace member, so both binaries land in $out/bin and
  # agnt-core is compiled once.
  combinedPackages = inputs.rustnix.lib.rust.buildTargetOutputs {
    inherit
      cargoToml
      cargoLock
      overlays
      pkgs
      system
      installData
      supportedTargets
      ;
    nixpkgs = inputs.nixpkgs;
    src = ../.;
    workspaceMember = null;
    packageName = "nu-agent";
    archiveAndHash = false;
    nativeBuildInputs = [pkgs.pkg-config pkgs.gitMinimal pkgs.cacert];
  };

  # Archive the combined derivation: .tgz with both binaries plus hashes.
  archivePackages = inputs.rustnix.lib.rust.buildTargetOutputs {
    inherit
      cargoToml
      cargoLock
      overlays
      pkgs
      system
      installData
      supportedTargets
      ;
    nixpkgs = inputs.nixpkgs;
    src = ../.;
    workspaceMember = null;
    packageName = "archive";
    archiveAndHash = true;
    nativeBuildInputs = [pkgs.pkg-config pkgs.gitMinimal pkgs.cacert];
  };

  # Named packages symlink into the combined derivation. They reference the
  # same store path, so Nix does not rebuild any Rust code.
  plugin = pkgs.runCommand "nu-agent-plugin" {} ''
    mkdir -p $out/bin
    ln -s ${combinedPackages.nu-agent}/bin/nu_plugin_agent $out/bin/nu_plugin_agent
  '';
  agent = pkgs.runCommand "nu-agent-cli" {} ''
    mkdir -p $out/bin
    ln -s ${combinedPackages.nu-agent}/bin/agent $out/bin/agent
  '';
in {
  # Export all package outputs. `default` is pinned to the combined binary
  # derivation so `nix build .#default` yields both binaries in $out/bin.
  packages =
    combinedPackages
    // archivePackages
    // {
      inherit plugin agent;
      default = combinedPackages.nu-agent;
    };

  # Export apps
  apps = {
    default = {
      type = "app";
      program = "${combinedPackages.nu-agent}/bin/agent";
    };
  };
}
