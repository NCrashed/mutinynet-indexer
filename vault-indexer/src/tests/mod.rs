use core::time::Duration;
use std::sync::Arc;
use std::thread;

use serial_test::serial;

use crate::indexer::{Indexer, Network, NodeStatus};

/// Mutiny signet local node (run with `start-regtest`)
const NODE_ADDRESS: &'static str = "127.0.0.1:18444";

#[test]
#[serial]
fn node_connection() {
    // Configure indexer and prepare to run
    let indexer = Arc::new(
        Indexer::builder()
            .network(Network::Signet)
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
    
    // Wait until node is connected
    wait_until(3, Duration::from_secs(1), || {
        indexer.node_status() == NodeStatus::Connected
    });
}

/// Helper that polls the function for `count` times and waits for `delay` between calls.
/// If the `body` returns `true`, stops polling and test continues, else panics.
fn wait_until<F>(count: u32, delay: Duration, mut body: F)
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
