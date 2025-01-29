#! /usr/bin/env nix-shell
#! nix-shell -i bash -Q shell.nix
DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
RUST_LOG="info,vault_indexer::db=info" cargo run --release -- --address=127.0.0.1:18444 "$@"
