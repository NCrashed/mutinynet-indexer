mod db;
mod framework;

use framework::*;

use crate::NodeStatus;
use core::time::Duration;
use serial_test::serial;

#[test]
#[serial]
fn node_connection() {
    let indexer = init_indexer();
    // Wait until node is connected
    wait_until(3, Duration::from_secs(1), || {
        indexer.node_status() == NodeStatus::Connected
    });
}

#[test]
#[serial]
fn node_sync_headers() {
    let indexer = init_indexer();
    // Wait until node is connected
    wait_until(3, Duration::from_secs(1), || {
        indexer.node_status() == NodeStatus::Connected
    });
    // Wait until we have non zero height of downloaded headers
    wait_until(3, Duration::from_secs(1), || {
        indexer.chain_height().unwrap() > 0
    });
}

#[test]
#[serial]
fn node_scan_process() {
    let indexer = init_indexer();
    // Wait until node is connected
    wait_until(3, Duration::from_secs(1), || {
        indexer.node_status() == NodeStatus::Connected
    });
    // Wait until we have non zero height of downloaded headers
    wait_until(3, Duration::from_secs(1), || {
        indexer.chain_height().unwrap() > 0
    });
    // Wait until we have scanned several blocks
    wait_until(3, Duration::from_secs(1), || {
        indexer.scanned_height().unwrap() > 0
    });
}