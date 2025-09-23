#INCLUDEFILE option("nix")
{
  pkgs ? import <nixpkgs> { },
  archname ? "x86_64-linux-gnu",
}:
let
in
pkgs.stdenv.mkDerivation rec {
  name = "esp-riscv32-gcc";
  version = "15.1.0_20250607";

  src = pkgs.fetchzip {
    url = "https://github.com/espressif/crosstool-NG/releases/download/esp-${version}/riscv32-esp-elf-${version}-${archname}.tar.xz";
    hash = "sha256-UEIlYiHqJDZ/S1bEwh47OcV5C8megc4F05n5Bh+vVM0=";
  };

  outputs = [ "out" ];

  installPhase = ''
    mkdir -p $out
    cp -r ./* $out/
  '';
}
