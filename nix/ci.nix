# CI shell for nu-agent - minimal tooling for builds
{
  pkgs,
  inputs,
  system,
}: let
  fenix = inputs.fenix.packages.${system};
in
  pkgs.mkShellNoCC {
    name = "nu-agent-ci";

    buildInputs = [
      # Rust toolchain (stable)
      fenix.stable.toolchain
    ];

    shellHook = ''
      echo "CI Testing Environment"
      echo "Rust: $(rustc --version)"
    '';
  }
