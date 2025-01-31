use bitcoin::consensus::Decodable;
use bitcoin::Transaction;
use core::assert_eq;
use ordinals::*;
use serial_test::serial;
use std::io::Cursor;

use crate::vault::UNIT_RUNE_ID;

/// Testing transaction that creates the vault utxo with runestone
/// https://mutinynet.com/tx/a1e204ea58e22030f4342cfdf36be49d4893afea2b65c098439fca36d3bebe0e
const OPEN_VAULT_TX_PHASE1: &str = "020000000001023f12c12a0dccc47970b437ef41e5a522ab7b51a90af366d28df0338ddcd66a440000000000ffffffff0e98a35da5a4862f7bef5c4e7d4c6f7ded1da930996f1a1c6cc7d7319505ec010000000000ffffffff0414270000000000002251207017dbe1bf7cbb61a9128e09df3668a433a023955e3e437565678dd2f976ed150e1a0f000000000022512037ce9992e6fdac01d0308a7b04d199ead0a3390fc6cff8a356b7ca698165cfa110270000000000002251201903b10c266e19425489d038a5b1e92f3633c3138a10c5c58957688e545e818700000000000000000b6a5d0800b89c5d01a052020140f849d9dcf3e7e0c16846e3516eafc13308d18a665b80eb389ca51c72e20437e837ff53a1d5a77a355b0172f04de5159ecb6ebaf947cbe9c4d621491be0703a8a02483045022100d9459b1e521d6b0a8326a64f79b6229e88b8458a3c144e1391922817f1e1471f02205d92b88796dfb5526398a39c9764d2556b25ef155be727ab7559e558805948110121022453e6880d36c08a6a08c3c5ae22f9dc05b2ab0a0e617a63842647854d35d62e00000000";

/// Testing first phase for repay procedure that should contain UNIT amounts
/// https://mutinynet.com/tx/ae3949f226b1c23e152f91308b7e132bfd40605b4334ddc5412a37b229ee6f77
const REPAY_TX_PHASE1: &str = "02000000000103fb474cb2acda3e15ec3046bed725056ae0cd4ed1d6b566c8df424d84315045710000000000ffffffff0ebebed336ca9f4398c0652beaaf93489de46bf3fd2c34f43020e258ea04e2a10200000000ffffffffd8b11e8a6d3e8d425cf54ab5e5f9d1ab37b7f5e7f4740037fc2ca05e27ceec410100000000ffffffff0414270000000000002251207017dbe1bf7cbb61a9128e09df3668a433a023955e3e437565678dd2f976ed15dbb80d00000000002251203137e6511517ea157d91b2bbaa717c4e2903c0500443f531409bf7cb62d9a4ec10270000000000002251201903b10c266e19425489d038a5b1e92f3633c3138a10c5c58957688e545e818700000000000000000b6a5d0800b89c5d01924e020140889c36a91ecfb6e13a62143e03744fdd78d3800414d0be4974df676e1278f0477fddc8ff11c0d59b253170e042cf287f75d4fb746dbc40529f7d7ee4c2a7ccd70140a63fa18a167f64eb46259c92caf90e6a469f64e65ae216af424a789477d71d9e967993c5e7c58ae2c9213a4c8aa973d5d5c2245ab6704112d144a74e207fab6f0247304402201c96be88523ee80d089e9fc2f432daea7bdf9be8f4e138572b696ca2e018ae8c02206ab3afa3e82088f6cccb22139d98fefff0d2dabb8fc0c8f7101fc9761d3ec2830121022453e6880d36c08a6a08c3c5ae22f9dc05b2ab0a0e617a63842647854d35d62e00000000";

/// Testing first phase for borrow procedure that should contain UNIT amounts
/// https://mutinynet.com/tx/75d57033461d130ca609cc390c309a65d77de97c3b4f4cea2dee1e175dd048c3
const BORROW_TX_PHASE1: &str = "0200000000010244326d9d5e8c337c1e678af55768b4c21b05a0bc3bfe652327352a4a75facea90000000000ffffffff06ac817566f12723d31f2a21d2f562e22c9b939945bd78af7820ae1598ec183e0100000000ffffffff0414270000000000002251207017dbe1bf7cbb61a9128e09df3668a433a023955e3e437565678dd2f976ed15f28e0d00000000002251203137e6511517ea157d91b2bbaa717c4e2903c0500443f531409bf7cb62d9a4ec10270000000000002251201903b10c266e19425489d038a5b1e92f3633c3138a10c5c58957688e545e818700000000000000000b6a5d0800b89c5d019a06020140c5d9e6f91530e7a3bea2fb4383925d3177ba34a2564b1abf10e718848cd24b6891a018cf9825233dd5b42512c5b4c1ceeeb3aba88c8095bfcf52ba0d433ce3ba02473044022073ff6a3f2bd7e72fbfe11ecb239a9747044c08ef6efacac37199734c2b5ceffd022002c7d9a35739484615dadd8280bef6a63a6f03103ef63c298facc83df11907160121022453e6880d36c08a6a08c3c5ae22f9dc05b2ab0a0e617a63842647854d35d62e00000000";

/// https://mutinynet.com/tx/d28e66a2e15b19281e8b972afc47ace0a5306dd07806e338b99542dcbc749ea0
const NOT_DETECTED_REPAY_TX: &str = "020000000001068e086b2d4ee402d897a190edaa8ccb7864591f12f780143dc1e1b0d69d8411760000000000ffffffff83eed9dfbe4836b2188683da300fe6c15d04cfb4283a403e0477c27615723a4a0200000000ffffffff97623ac2745fff886e4ba99e49dc04b708014d69ba2406a34e32f9a4e01bca0a0200000000ffffffff83b257ab4941412c62a74b329d80c92acf4e648198a2c3a4843c48111d09c0300200000000ffffffff15e1a3cde8c5fb9dfabf571f67bc23038f654ae4411acde225225aee075267380200000000ffffffff9d353020a5362da17e0c72860020396e69e03c61d3a58e287e6dbb279ef31e200100000017160014cb498fb76be92aeb22f8a2291b4748b21f40b67cffffffff0414270000000000002251207017dbe1bf7cbb61a9128e09df3668a433a023955e3e437565678dd2f976ed15d94326000000000022512070b49761faf5787540046e0374c49c18ef119d4950a126ae3b9e8c7c17ab396810270000000000002251206536f0d572602f476e5958f5505790157bc73ba0aa5b467c20eda6f7f3e10de800000000000000000a6a5d0700b89c5d0100020140d197e909a6273fa40904e21bf3b618fb886411dbd9092c6bac50ad0ca2ed3babdf3760398d4100fece6f7c7e7db756667d80b685c8dceafadadb4eda9917235e014050dee7812178adb4a761c1eedb7ac82fca97c35ae05d723cb3b574ecb46924e6f06896b24cd0cfb1cf96070e7a3a68498d65213fd192a8385534d7a321f409e001409f79bf4ab5ed5dcda092a36eaa06cd72a4270a0e7079c4373730d7278d3999b5f4a71cdf6adccd390dd4021ad05cb6d5939aa52bf241937fa6eceb2e2001491d0140afbd89220894f651d3f98aaf860c8205c1baa71228af5323a2eb7dd3fc19249844a041e972153d97fbf862f67f8fcdc7130cbbc1a8f2fd1cc606f1ca49938c7701408d2c0d154e03215fafdc7b848a3a9f0c0f7568600a4946fda6ba9e4389df882694adf73797c3207749b51048a0cdb2337b216937e368bf7de8c7b0a064d811280247304402207b5706548ace0856663157715cf6e0d37c3d3f505ae1e1e027069417cb99ce7b02207621dce834b70169e1885509fcbdf557bb2d5d5ab1ce38214bec6a7f45265474012102cf473dde22e4798c5b4e6fdef4f730833f3bd99de354bf56e4466732cd74b0df00000000";

#[test]
#[serial]
fn parse_open_vault_edict() {
    let tx =
        Transaction::consensus_decode(&mut Cursor::new(hex::decode(OPEN_VAULT_TX_PHASE1).unwrap()))
            .unwrap();
    if let Artifact::Runestone(artifact) = Runestone::decipher(&tx).unwrap() {
        // println!("{:#?}", artifact);
        let edict = artifact.edicts[0];
        assert_eq!(edict.id, UNIT_RUNE_ID);
        assert_eq!(edict.amount, 10528);
    } else {
        panic!("Runestone is not valid");
    }
}

#[test]
#[serial]
fn parse_repay_edict() {
    let tx = Transaction::consensus_decode(&mut Cursor::new(hex::decode(REPAY_TX_PHASE1).unwrap()))
        .unwrap();
    if let Artifact::Runestone(artifact) = Runestone::decipher(&tx).unwrap() {
        // println!("{:#?}", artifact);
        let edict = artifact.edicts[0];
        assert_eq!(edict.id, UNIT_RUNE_ID);
        assert_eq!(edict.amount, 10002);
    } else {
        panic!("Runestone is not valid");
    }
}

#[test]
#[serial]
fn parse_borrow_edict() {
    let tx =
        Transaction::consensus_decode(&mut Cursor::new(hex::decode(BORROW_TX_PHASE1).unwrap()))
            .unwrap();
    if let Artifact::Runestone(artifact) = Runestone::decipher(&tx).unwrap() {
        // println!("{:#?}", artifact);
        let edict = artifact.edicts[0];
        assert_eq!(edict.id, UNIT_RUNE_ID);
        assert_eq!(edict.amount, 794);
    } else {
        panic!("Runestone is not valid");
    }
}

#[test]
#[serial]
fn parse_repay_not_detected() {
    let tx = Transaction::consensus_decode(&mut Cursor::new(
        hex::decode(NOT_DETECTED_REPAY_TX).unwrap(),
    ))
    .unwrap();
    if let Artifact::Runestone(artifact) = Runestone::decipher(&tx).unwrap() {
        // println!("{:#?}", artifact);
        let edict = artifact.edicts[0];
        assert_eq!(edict.id, UNIT_RUNE_ID);
        assert_eq!(edict.amount, 0); // it has 0 runes!
    } else {
        panic!("Runestone is not valid");
    }
}
