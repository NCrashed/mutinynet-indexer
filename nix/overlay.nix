self: super: rec {
    # I require old version as mutiny fork won't compile with new 2.2.8 version: https://bugs.gentoo.org/934821
    miniupnpc-2_2_7 = self.callPackage ./miniupnpc.nix {};
    bitcoind-mutiny = self.callPackage ./bitcoind-mutiny.nix { withGui = false; miniupnpc = miniupnpc-2_2_7; };
}