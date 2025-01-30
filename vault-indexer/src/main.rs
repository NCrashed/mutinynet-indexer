use clap::Parser;
use core::result::Result;
use log::*;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use vault_indexer::*;

#[derive(Debug, Error)]
enum Error {
    #[error("Indexer failure: {0}")]
    Indexer(#[from] indexer::Error),
    #[error("Service failure: {0}")]
    Service(#[from] service::Error),
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of network to work with.
    #[arg(short, long, default_value_t = Network::Mutinynet)]
    network: Network,

    /// Address of node ip:port or domain:port. Default is remote Mutiny net node.
    #[arg(short, long, default_value = "45.79.52.207:38333")]
    address: String,

    /// Path to database of the indexer
    #[arg(short, long, default_value = "indexer.sqlite")]
    database: PathBuf,

    /// Amount of blocks to query per batch
    #[arg(short, long, default_value_t = 500)]
    batch: u32,

    /// The height of blockhcain we start scanning from. Note that we still need download all
    /// headers from the genesis.
    #[arg(short, long, default_value_t = 1527651)]
    start_height: u32,

    /// Websocket service bind address
    #[arg(short, long, default_value = "127.0.0.1:39987")]
    websocket_address: String,

    /// Start scanning blocks from begining (--start-height), doesn't
    /// redownload headers.
    #[arg(long)]
    rescan: bool,
}

fn main() -> Result<(), Error> {
    if env::var("RUST_LOG").is_err() {
        let _ = env::set_var("RUST_LOG", "debug");
    }
    env_logger::init();
    let args = Args::parse();

    debug!("Configuring indexer");
    let m_indexer = Indexer::builder()
        .network(args.network)
        .node(&args.address)
        .db(&args.database)
        .batch_size(args.batch)
        .start_height(args.start_height)
        .rescan(args.rescan)
        .build();

    let indexer = match m_indexer {
        Err(e) => {
            error!("Failed to configure indexer: {e}");
            return Err(e.into());
        }
        Ok(indexer) => Arc::new(indexer),
    };

    debug!("Spawn weboscket service");
    match service::start_websocket_server(indexer.clone(), &args.websocket_address) {
        Err(e) => {
            error!("Failed to start websocket service: {e}");
            return Err(e.into());
        }
        _ => (),
    }

    debug!("Start indexer");
    match indexer.run() {
        Err(e) => {
            error!("Indexing fatal error: {e}");
            return Err(e.into());
        }
        Ok(_) => Ok(()),
    }
}
