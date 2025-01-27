use std::io::Cursor;

use crate::cache::headers::HeadersCache;
use crate::db::*;
use crate::tests::framework::*;
use crate::Network;
use bitcoin::block::Header;
use bitcoin::consensus::Decodable;
use serial_test::serial;

const HEADER_HEIGHT_1: &'static str = "00000020f61eee3b63a380a477a063af32b2bbc97c9ff9f01f2c4225e973988108000000011ba17baed1cacfb3793ba391383c305e401b3c54b3ce611c05d8b29927ad9e023d2f64ae77031ec0db7a01";
const HEADER_HEIGHT_2: &'static str = "00000020f95429cd19fc22dac910fce4fe26a3580577fc5efcaf4eb2a9a0935885020000899658c98e65e369651736e8a5c206ab318260ddaaa5ca337644b074e6209a71363d2f64ae77031ee1b25700";
const HEADER_HEIGHT_3: &'static str = "0000002096e0e15c52707f525d4b40bac68dd2712e9f032d374157e786bac0314d01000093f673cea9778c92f3a6fc64306144f055852542e2ebd72edbef3d3000134b4b5a3d2f64ae77031ea1542500";

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

    let test_header = mk_header(HEADER_HEIGHT_1);

    db.store_block_header(test_header).unwrap();
    let read_header = db
        .load_block_header(test_header.block_hash())
        .unwrap()
        .unwrap();
    assert_eq!(test_header, read_header.header);
}

#[test]
#[serial]
fn db_main_tip() {
    let db = init_db();
    let mut cache = HeadersCache::load(&db).unwrap();

    let test_header1 = mk_header(HEADER_HEIGHT_1);
    let test_header2 = mk_header(HEADER_HEIGHT_2);

    cache.update_longest_chain(&[test_header1, test_header2]).unwrap();
    cache.store(&db).unwrap();
    
    let tip_hash = db.get_main_tip().unwrap();
    assert_eq!(test_header2.block_hash(), tip_hash);
}

#[test]
#[serial]
fn db_orphans_ordering() {
    let db = init_db();
    let mut cache = HeadersCache::load(&db).unwrap();

    let test_header1 = mk_header(HEADER_HEIGHT_1);
    let test_header2 = mk_header(HEADER_HEIGHT_2);

    // Header 2 arrives faster than header 1 (somehow)
    cache.update_longest_chain(&[test_header2]).unwrap();
    cache.update_longest_chain(&[test_header1]).unwrap();
    cache.store(&db).unwrap();
    
    let tip_hash = db.get_main_tip().unwrap();
    assert_eq!(test_header2.block_hash(), tip_hash);
}

#[test]
#[serial]
fn db_fork_inactive() {
    let db = init_db();
    let mut cache = HeadersCache::load(&db).unwrap();

    let test_header1 = mk_header(HEADER_HEIGHT_1);
    let test_header2 = mk_header(HEADER_HEIGHT_2);

    let fork_header1 = fake_fork_mine(test_header1);
    
    cache.update_longest_chain(&[test_header1, test_header2]).unwrap();
    cache.update_longest_chain(&[fork_header1]).unwrap();
    cache.store(&db).unwrap();
    
    let tip_hash = db.get_main_tip().unwrap();
    assert_eq!(test_header2.block_hash(), tip_hash);
}

#[test]
#[serial]
fn db_fork_active() {
    let db = init_db();
    let mut cache = HeadersCache::load(&db).unwrap();

    let test_header1 = mk_header(HEADER_HEIGHT_1);
    let test_header2 = mk_header(HEADER_HEIGHT_2);

    let fork_header1 = fake_fork_mine(test_header1);
    let mut fork_header2 = test_header2;
    fork_header2.prev_blockhash = fork_header1.block_hash();
    let fork_header2 = fake_fork_mine(fork_header2);
    
    cache.update_longest_chain(&[test_header1]).unwrap();
    cache.update_longest_chain(&[fork_header1, fork_header2]).unwrap();
    cache.store(&db).unwrap();
    
    let tip_hash = db.get_main_tip().unwrap();
    assert_eq!(fork_header2.block_hash(), tip_hash);
}

fn fake_fork_mine(mut header: Header) -> Header {
    let start_work = header.work();
    loop {
        header.nonce += 1;
        if header.work() >= start_work {
            println!("Header {}, mined fake nonce : {}", header.block_hash(), header.nonce);
            break;
        }
    }
    header
}

fn mk_header(hex: &str) -> Header {
    let header_bytes = hex::decode(hex).expect("correct hex encoded header");
    Header::consensus_decode(&mut Cursor::new(&header_bytes)).expect("decoded header from bytes")
}
