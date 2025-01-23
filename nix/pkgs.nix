# To update nix-prefetch-git https://github.com/NixOS/nixpkgs
import ((import <nixpkgs> {}).fetchFromGitHub {
  owner = "NixOS";
  repo = "nixpkgs";
  rev = "1809387d09edc8bd835f1adb79fb5e9ccc9dffe3";
  sha256  = "1pl1fpsbgpxv94n4xxx7dxvla1nkhnplvqxvmx4b6spcr5rpf16l";
})
