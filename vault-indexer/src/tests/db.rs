use std::io::Cursor;

use crate::tests::framework::*;
use crate::Network;
use bitcoin::block::Header;
use bitcoin::consensus::Decodable;
use serial_test::serial;

#[test]
#[serial]
fn db_genesis() {
    let db = init_db();

    let genesis_header = Network::Mutinynet.genesis_header();
    let read_header = db
        .load_block_header(genesis_header.block_hash())
        .unwrap()
        .unwrap();
    assert_eq!(genesis_header, read_header.header);
}

#[test]
#[serial]
fn db_store_header() {
    let db = init_db();

    let test_header_bytes = hex::decode("00000020f61eee3b63a380a477a063af32b2bbc97c9ff9f01f2c4225e973988108000000011ba17baed1cacfb3793ba391383c305e401b3c54b3ce611c05d8b29927ad9e023d2f64ae77031ec0db7a01").unwrap();
    let test_header = Header::consensus_decode(&mut Cursor::new(&test_header_bytes)).unwrap();

    db.store_block_header(test_header).unwrap();
    let read_header = db
        .load_block_header(test_header.block_hash())
        .unwrap()
        .unwrap();
    assert_eq!(test_header, read_header.header);
}
