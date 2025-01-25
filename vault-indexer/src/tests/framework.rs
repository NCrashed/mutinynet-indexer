use core::time::Duration;
use std::sync::Arc;
use std::sync::Once;
use std::thread;

use log::LevelFilter;

use crate::{Indexer, Network};

/// Mutiny signet local node (run with `start-regtest`)
const NODE_ADDRESS: &'static str = "127.0.0.1:18444";

static INIT: Once = Once::new();

pub fn init_indexer() -> Arc<Indexer> {
    INIT.call_once(|| {
        // Configure logging
        env_logger::builder()
            .filter(None, LevelFilter::Trace)
            .init();
    });

    // Configure indexer and prepare to run
    let indexer = Arc::new(
        Indexer::builder()
            .network(Network::Mutinynet)
            .node(NODE_ADDRESS)
            .build()
            .expect("Indexer configured"),
    );
    // Run it asynchronously in background
    thread::spawn({
        let indexer = indexer.clone();
        move || {
            indexer.run().expect("Indexer start failure");
        }
    });

    indexer
}

/// Helper that polls the function for `count` times and waits for `delay` between calls.
/// If the `body` returns `true`, stops polling and test continues, else panics.
pub fn wait_until<F>(count: u32, delay: Duration, mut body: F)
where
    F: FnMut() -> bool,
{
    for _ in 0..count {
        let res = body();
        if res {
            return ();
        }
        thread::sleep(delay);
    }
    panic!("Failed to finish action in wait_until in time");
}
