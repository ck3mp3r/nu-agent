# CI shell for nu-agent - minimal tooling for builds
{
  pkgs,
  inputs,
  system,
}: let
  toolchain = inputs.rustnix.lib.rust.mkToolchain {inherit system;};
in
  pkgs.mkShellNoCC {
    name = "nu-agent-ci";

    buildInputs = [
      # Rust toolchain (stable)
      toolchain
    ];

    shellHook = ''
      echo "CI Testing Environment"
      echo "Rust: $(rustc --version)"
    '';
  }
