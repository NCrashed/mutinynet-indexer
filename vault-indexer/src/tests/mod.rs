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
