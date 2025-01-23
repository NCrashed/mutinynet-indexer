with import ./nix/pkgs.nix {
  overlays = [ (import ./nix/overlay.nix) ];
};
stdenv.mkDerivation rec {
  name = "rust-env";
  env = buildEnv { name = name; paths = buildInputs; };

  buildInputs = [
    bitcoind-mutiny
    rustup
    sqlite
  ];
}
