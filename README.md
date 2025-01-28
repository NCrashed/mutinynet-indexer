# Mutiny signet indexer

## Hacking process

You will need the [Nix](nixos.org) package manager in your `PATH`. Other system deps and toolchains will be fetched by the nix.

### Start the Mutiny node

The step is optional, you can use any Mutinynet node (for instance the official one `45.79.52.207:38333`). But, the indexing process will be much faster and robust with the local node:

``` bash
./start-signet
```

## Repo structure

- `nix` - contains all [Nix](nixos.org) configurations required to reproducibly build this repository and the Mutiny full node.
    * `bitcoind-mutiny.nix` - Nix derivation for [Mutiny fork](https://github.com/benthecarman/bitcoin/releases) of Bitcoin Core.
    * `miniupnpc.nix` - Nix derivation for version 2.2.7 of the IGD client. The current version (2.2.8) doesn't compile with this Bitcoin Core fork ([bug report](https://bugs.gentoo.org/934821)). While we could patch the node, we prefer sticking to the same version used by the broader Mutiny community. 
    * `overlay.nix` - Gathers all packages into a single [Nix overlay](https://nixos.wiki/wiki/Overlays) injected into `nixpkgs`.
    * `pkgs.nix` - pinned version of `nixpkgs` used by the project, ensuring reproducible builds even years after the last commit.
- `btc-cli` - A script that lets you interact with the local Mutiny node without specifying extra parameters.
- `clean-signet` - A script that removes the local Mutiny node's state, allowing you to start from scratch.
- `rust-toolchain.toml` - Pins the Rust toolchain for reproducible builds.
- `shell.nix` - Contains a developing and testing environments that includes the Rust toolchain and Mutiny node.
- `start-signet` - Launches the local Mutiny node for testing.
