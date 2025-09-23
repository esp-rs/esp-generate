#INCLUDEFILE option("nix")
{
  pkgs ? import <nixpkgs> { },
  archname ? "x86_64-linux-gnu",
}:
let
in
pkgs.stdenv.mkDerivation rec {
  name = "esp-xtensa-gcc";
  version = "15.1.0_20250607";

  src = pkgs.fetchzip {
    url = "https://github.com/espressif/crosstool-NG/releases/download/esp-${version}/xtensa-esp-elf-${version}-${archname}.tar.xz";
    hash = "sha256-OLm7JQeeFdvTbga1s6D0aMFVGWjNhG023t/aHX+xxz0=";
  };

  outputs = [ "out" ];

  installPhase = ''
    mkdir -p $out
    cp -r ./* $out/
  '';
}
