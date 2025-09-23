#INCLUDEFILE option("nix")
{
  pkgs ? import <nixpkgs> { },
}:
let
  # Uses this folder as the source for esp-rs-nix (in-repo nix-shell)
  esp-rs-src = pkgs.lib.sources.cleanSource ./.;

  # Examples for out-of-repo nix-shell usage below.
  #
  # # Use the latest release of esp-rs-nix
  # esp-rs-src = builtins.fetchTarball "https://github.com/leighleighleigh/esp-rs-nix/archive/master.tar.gz";
  #
  # # Use a pinned release of esp-rs-nix, at commit '0c3fa7245d38019e60c4ae56b2e98465c1b8a5dc'
  # esp-rs-src = pkgs.fetchFromGitHub {
  #   owner = "leighleighleigh";
  #   repo = "esp-rs-nix";
  #   rev = "0c3fa7245d38019e60c4ae56b2e98465c1b8a5dc";
  #   hash = "sha256-b5kb6gxqutHySWEoweNfKbZw1r7DkwqRC39RWsyFSLU=";
  # };

  # Call the package from wherever the source was obtained
  esp-rs = pkgs.callPackage "${esp-rs-src}/esp-rs/default.nix" { };
in
pkgs.mkShell rec {
  name = "esp-rs-nix";

  # You may wish to change these build inputs for your application
  buildInputs = [
    esp-rs
    pkgs.rust-analyzer
    pkgs.rustup
    pkgs.espflash
    pkgs.pkg-config
    pkgs.stdenv.cc
    pkgs.systemdMinimal
  ];

  shellHook = ''
    # Add a prefix 'esp-rs' to the shell prompt
    export PS1="(esp-rs)$PS1"

    # This variable is important - it tells rustup where to find the esp toolchain,
    # without needing to copy it into your local ~/.rustup/ folder.
    export RUSTUP_TOOLCHAIN=${esp-rs}
  '';
}
