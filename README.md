# Mutiny signet indexer

## Hacking process

You will need the [Nix](nixos.org) package manager in your `PATH`. Other system deps and toolchains will be fetched by the nix.

### Start the Mutiny node

The step is optional, you can use any Mutinynet node (for instance the official one `45.79.52.207:38333`). But, the indexing process will be much faster and robust with the local node:

``` bash
./start-signet
```

### Start the indexer 

The simpliest form of running the indexer is using the script below. Nix will fetch Rust toolchain and build the indexer from source. The configuration targets the local node on the Mutiny Signet.

``` bash 
./run-indexer
```

You can restart scanning with:
``` bash
./run-indexer --rescan
```

Or, you can connect to the external public Mutiny node:
```bash
./run-public
```

### Test WebSocket service 

The websocket service is started on the `ws://127.0.0.1:39987` by default. You can adjust this with command line arguments, see `./run-indexer --help`. 

To test the endpoints one can use `./run-client` script that uses [websocat]() to connect to the local indexer on the default port. You should type calls in the format `{"method": "range_history_all"}`. The available methods are listed bellow:
* `range_history_all`: Return all vault-related transactions within a specified time range (optional start and end timestamps). Example: 
```json
{"method": "range_history_all", "timestamp_start": 1738113524, "timestamp_end": 1738225126 }
```
* `vault_history_tx`: Return all transactions for a given vault within a specified time range. Example:
```json 
{"method": "vault_history_tx", "vault_open_txid":"a9cefa754a2a35272365fe3bbca0051bc2b46857f58a671e7c338c5e9d6d3244","timestamp_start": 1738113524, "timestamp_end": 1738225126 }
```
* `action_history`: Return aggregated action data over specified time spans (e.g., daily, weekly).
* `overall_volume`: Return aggregated volume metrics (BTC and units) over a specified time span.


## Repo structure

- `src/cache` - contains in-memory cache for Bitcoin headers chains and algorithms for fork detection, chain reorganization.
- `src/db` - contains all SQlite related actions, schemas and queries.
- `src/indexer/mod.rs` - contains user API and blockchain traversal logic.
- `src/indexer/node.rs` - contains logic to control TCP connection to other nodes.
- `src/tests` - contains integration and unit tests. 
- `src/vault` - contains domain types for Vault transactions and parser from hosted Bitcoin transactions.
- `src/service.rs` - contains WebSocket service that is decoupled from the indexer.
- `src/main.rs` - collects all components in one place using the indexer and the WebSocket service as libraries.

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
- `run-client` - Launches the Websocket client for testing.
- `run-indexer` - Launches the local indexer from sources that targets local Mutiny node.
- `run-public` - Launches the local indexer from sources that targets public Mutiny node.

