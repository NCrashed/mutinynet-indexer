# Mutiny signet indexer

## Demo Assumptions

- The syncing process **DOES** include fork detection and a proper reorganization algorithm to track the chain with the largest accumulated PoW. However, the demo **DOESN’T** deactivate any previously scanned transactions that are later dropped. That aspect is beyond the scope of this demo.

- I have made some bold assumptions about the structure of vault transactions. For instance:
  - The open transaction always uses the 3rd output for locking collateral, while other transaction types use the 1st output for collateral.
  - Collateral is always placed in the transaction’s first input.
  - Each transaction includes only one vault operation.
  - UTXO connector is placed in 2nd slot of inputs and leads to the phase 1 transaction with UNIT runestone.

- The indexer also search for any UNIT related transactions and saves them to provide proper UNIT volumes for Vault transactions. 

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
The syncing is done in two phases. First, headers of the main chain is downloaded. After reaching the tip, the scanning progress will start from `start-height` block height.

You can restart scanning with:
``` bash
./run-indexer --rescan
```

Or, you can connect to the external public Mutiny node:
```bash
./run-public
```

Paramers are described in CLI `--help` output of the indexer: 
```
Usage: vault-indexer [OPTIONS]

Options:
  -n, --network <NETWORK>
          Name of network to work with
          
          [default: mutinynet]

          Possible values:
          - bitcoin:   Mainnet Bitcoin
          - testnet:   Bitcoin's testnet network
          - testnet4:  Bitcoin's testnet4 network
          - signet:    Bitcoin's signet network
          - mutinynet: Mutiny custom signet network
          - regtest:   Bitcoin's regtest network

  -a, --address <ADDRESS>
          Address of node ip:port or domain:port. Default is remote Mutiny net node
          
          [default: 45.79.52.207:38333]

  -d, --database <DATABASE>
          Path to database of the indexer
          
          [default: indexer.sqlite]

  -b, --batch <BATCH>
          Amount of blocks to query per batch
          
          [default: 500]

  -s, --start-height <START_HEIGHT>
          The height of blockhcain we start scanning from. Note that we still need download all headers from the genesis
          
          [default: 1527651]

  -w, --websocket-address <WEBSOCKET_ADDRESS>
          Websocket service bind address
          
          [default: 127.0.0.1:39987]

      --rescan
          Start scanning blocks from begining (--start-height), doesn't redownload headers

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

### Test WebSocket service 

The websocket service is started on the `ws://127.0.0.1:39987` by default. You can adjust this with command line arguments, see `./run-indexer --help`. 

To test the endpoints one can use `./run-client` script that uses [websocat]() to connect to the local indexer on the default port. You should type calls in the format `{"method": "range_history_all"}`. 

Real-time notification can be tested in two ways. Either connect to the node when it is syncing or post a vault transaction to the network while listening the WebSocket. You should see the notification in the following format (after prettying):
``` json
{
  "NewTranscation": {
    "vault_id": "2909c85ad5fa97f9c734124f3504a79c8a82a31db3b1fd8183e43fd9a24c6703",
    "txid": "5cf2948536a902ce000507f2bd859192d672169b680230d3e49de559788846c8",
    "op_return_output": 2,
    "version": "1_legacy",
    "action": "borrow",
    "balance": 79817,
    "oracle_price": 56127,
    "oracle_timestamp": 1731259926,
    "liquidation_price": null,
    "liquidation_hash": null,
    "block_hash": "0000001faaf7382bcf78b2d7d731c87487cbe6ed17ccc02ed530c9b99f8186b5",
    "height": 1590395,
    "tx_url": "https://mutinynet.com/tx/5cf2948536a902ce000507f2bd859192d672169b680230d3e49de559788846c8",
    "btc_custody": 1723510,
    "unit_volume": 2988,
    "btc_volume": 0,
    "prev_tx": "https://mutinynet.com/tx/96932d3925125eb9441692605a1cd8a693d6aff1094cdfb88e97f2e3df4acbc6"
  }
}
```
Notes about the format:
- `vault_id` is hash of the opening transcation;
- `balance`, `unit_volume`, `oracle_price` are provided in their minimal units (as encoded in op_return payload);
- BTC units are always in sats.

The available call methods are listed bellow:
* `range_history_all`: Return all vault-related transactions within a specified time range (optional start and end timestamps). Example: 
```json
{"method": "range_history_all", "timestamp_start": 1738113524, "timestamp_end": 1738225126 }
```
You should expect the following result: 
``` json
{
  "AllHistory": [
    {
      "vault_id": "9d40a831d2ac425c04e21a2d678b234beed8913dfb290a410a3a0e14e7e2f4d8",
      "txid": "0f442831c3f1ac79d62d3c4ed2afef1f8d9c44a58f34f4b222e6abc7f6721e6f",
      "op_return_output": 2,
      "version": "1_legacy",
      "action": "borrow",
      "balance": 102006,
      "oracle_price": 383153,
      "oracle_timestamp": 1738116742,
      "liquidation_price": null,
      "liquidation_hash": null,
      "block_hash": "0000035fb9375d720b5c950e1b4113eacf16e306a8810a3d1197232a7bf29ded",
      "height": 1810807,
      "tx_url": "https://mutinynet.com/tx/0f442831c3f1ac79d62d3c4ed2afef1f8d9c44a58f34f4b222e6abc7f6721e6f",
      "btc_custody": 11686787,
      "unit_volume": -1971,
      "btc_volume": 0,
      "prev_tx": "https://mutinynet.com/tx/a96f34bffc5fb1427f28b707d1ee524b01c564da03c5ff7a2cdaaf4949a4d1e0"
    }
  ]
}
```

* `vault_history_tx`: Return all transactions for a given vault within a specified time range. Example:
```json 
{"method": "vault_history_tx", "vault_open_txid":"a9cefa754a2a35272365fe3bbca0051bc2b46857f58a671e7c338c5e9d6d3244","timestamp_start": 1738113524, "timestamp_end": 1738225126 }
```
Expected response:
```json
{
  "VaultHistory": [
    {
      "vault_id": "a9cefa754a2a35272365fe3bbca0051bc2b46857f58a671e7c338c5e9d6d3244",
      "txid": "4012016d9527bfb3bef9c51dded9123d812f9c259961d29ef7e5bf17e358d741",
      "op_return_output": 2,
      "version": "1_legacy",
      "action": "deposit",
      "balance": 104274,
      "oracle_price": 13725,
      "oracle_timestamp": 1738202039,
      "liquidation_price": null,
      "liquidation_hash": null,
      "block_hash": "000001af5bfcef624a1047681eb3966ca2a42659fb9f7386b4390678c478e900",
      "height": 1813556,
      "tx_url": "https://mutinynet.com/tx/4012016d9527bfb3bef9c51dded9123d812f9c259961d29ef7e5bf17e358d741",
      "btc_custody": 2810335,
      "unit_volume": 9165,
      "btc_volume": 1209359,
      "prev_tx": "https://mutinynet.com/tx/cedd95445d2cc549788ca471cda405d78512380001e5ed496accdf812fb969ff"
    }
  ]
}
```

* `action_history`: Return aggregated action data over specified time spans (e.g., daily, weekly). Examples:
```json
{"method": "action_history", "action":"Open"}
{"method": "action_history", "action":"Open", "timespan":"Hour"}
{"method": "action_history", "action":"Deposit", "timespan":"Day"}
{"method": "action_history", "action":"Withdraw", "timespan":"Week"}
{"method": "action_history", "action":"Borrow", "timespan":"Week"}
{"method": "action_history", "action":"Repay", "timespan":"Month"}
```
Result:
```json
{
  "ActionHistory": [
    {
      "timestamp_start": 1729123200,
      "unit_volume": 130385,
      "btc_volume": 6010000
    },
    {
      "timestamp_start": 1729728000,
      "unit_volume": 1524555,
      "btc_volume": 53255112
    },
  ]
}
```
`timestamp_start` field mark the begining of each time bucket.

* `overall_volume`: Return aggregated volume metrics (BTC and units) over a specified time span.
```json
{"method": "overall_volume"}
```
Result:
```json
{"OveallVolume":{"btc_volume":12680920450,"unit_volume":73485324}}
```
Note: the withdraw volumes are subtracted from the total volume.

## Repo structure

- `vault-indexer` - the library and application in the same crate:
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

