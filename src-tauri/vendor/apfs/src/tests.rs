use super::*;
use std::io::{Cursor, Read, Seek, SeekFrom};

const BASIC: &[u8] = include_bytes!("../tests/data/basic.apfs");
const CASE_SENSITIVE: &[u8] = include_bytes!("../tests/data/case-sensitive.apfs");

#[test]
fn checkpoint_transaction_and_volume_slots_are_preserved() {
    let info = ApfsVolume::container_info(Cursor::new(BASIC)).unwrap();
    assert_eq!(info.transaction_id, 10);
    assert_eq!(info.size, 65536);
    assert_eq!(info.volume_slots, [(0, 100), (2, 101)]);
    let volumes = ApfsVolume::list_volumes(Cursor::new(BASIC)).unwrap();
    assert_eq!(volumes.len(), 2);
    assert_eq!(volumes[1].index, 2);
    assert!(volumes[1].encrypted);
    assert_eq!(volumes[0].role, 64);
    assert_eq!(
        volumes[0].uuid,
        core::array::from_fn(|index| 16 + index as u8)
    );
    assert!(matches!(
        ApfsVolume::open_index(Cursor::new(BASIC), 2),
        Err(ApfsError::Unsupported(_))
    ));
    assert!(matches!(
        ApfsVolume::open_index(Cursor::new(BASIC), 1),
        Err(ApfsError::NoVolume)
    ));
}

#[test]
fn full_streamed_and_seekable_reads_agree_for_clones_sparse_and_compressed_files() {
    let mut volume = ApfsVolume::open(Cursor::new(BASIC)).unwrap();
    let entries = volume.list_directory("/").unwrap();
    assert_eq!(entries.len(), 5);
    assert!(entries.iter().any(|entry| entry.name == "plain.bin"));
    assert!(
        !entries
            .iter()
            .any(|entry| entry.name == "old.bin" || entry.name == "future.bin")
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.name == "link")
            .unwrap()
            .link_target
            .as_deref(),
        Some("../plain.bin")
    );
    for name in [
        "plain.bin",
        "clone.bin",
        "hole.bin",
        "compressed.txt",
        "link",
    ] {
        let path = format!("/{name}");
        let complete = volume.read_file(&path).unwrap();
        let stat = volume.stat(&path).unwrap();
        assert_eq!(stat.size, complete.len() as u64);
        let mut streamed = Vec::new();
        assert_eq!(
            volume.read_file_to(&path, &mut streamed).unwrap(),
            stat.size
        );
        assert_eq!(streamed, complete);
        for offset in [0, 1, 8, 4095, 4096, 4999, 5000, 8192, u64::MAX] {
            let actual = volume.read_file_range(&path, offset, 30).unwrap();
            let expected = complete.get(offset as usize..).unwrap_or_default();
            assert_eq!(
                actual,
                expected[..expected.len().min(30)],
                "{path} offset={offset}"
            );
        }
        let mut by_id = Vec::new();
        volume
            .open_file_by_id(stat.oid)
            .unwrap()
            .read_to_end(&mut by_id)
            .unwrap();
        assert_eq!(by_id, complete);
        assert_eq!(volume.stat_by_id(stat.oid).unwrap().size, stat.size);
    }
    assert_eq!(
        volume.read_file("/clone.bin").unwrap(),
        volume.read_file("/plain.bin").unwrap()
    );
    assert_eq!(
        volume.read_file("/compressed.txt").unwrap(),
        b"hello compressed"
    );
    assert_eq!(volume.read_file("/hole.bin").unwrap(), vec![0; 8192]);
    let stat = volume.stat("/plain.bin").unwrap();
    assert_eq!(
        (stat.parent_oid, stat.private_id, stat.uid, stat.gid),
        (2, 70, 501, 20)
    );
    assert_eq!(stat.create_time, -1);
    assert!(volume.open_file("/").is_err());
}

#[test]
fn attributes_are_independent_seekable_streams() {
    let mut volume = ApfsVolume::open(Cursor::new(BASIC)).unwrap();
    assert_eq!(
        volume
            .get_xattr("/plain.bin", "user.small")
            .unwrap()
            .unwrap(),
        b"value"
    );
    assert_eq!(
        volume
            .get_xattr("/plain.bin", "user.stream")
            .unwrap()
            .unwrap(),
        b"ATTR!!"
    );
    assert!(volume.get_xattr("/plain.bin", "absent").unwrap().is_none());
    let info = volume
        .xattr_info("/plain.bin", "user.stream")
        .unwrap()
        .unwrap();
    assert_eq!(info.stream_id, Some(80));
    assert_eq!(info.size, 6);
    assert_eq!(info.extents[0].value.phys_block_num, 14);
    let mut attribute = volume
        .open_xattr("/plain.bin", "user.stream")
        .unwrap()
        .unwrap();
    attribute.seek(SeekFrom::Start(2)).unwrap();
    let mut bytes = Vec::new();
    attribute.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"TR!!");
}

#[test]
fn both_directory_key_layouts_and_volume_name_policies_work() {
    let mut insensitive = ApfsVolume::open(Cursor::new(BASIC)).unwrap();
    assert_eq!(insensitive.read_file("/PLAIN.BIN").unwrap().len(), 5000);
    let mut sensitive = ApfsVolume::open(Cursor::new(CASE_SENSITIVE)).unwrap();
    assert_eq!(sensitive.list_directory("/").unwrap().len(), 5);
    assert_eq!(sensitive.read_file("/plain.bin").unwrap().len(), 5000);
    assert!(sensitive.read_file("/PLAIN.BIN").is_err());
}

#[test]
fn omap_cache_and_lookup_obey_the_same_transaction_and_tombstone_rules() {
    let mut source = Cursor::new(BASIC.to_vec());
    for (xid, expected) in [(5, 9), (10, 10), (11, 11)] {
        let tree = omap::OmapTree { root: 8, xid };
        assert_eq!(
            omap::omap_lookup(&mut source, tree, 4096, 200).unwrap(),
            expected
        );
        assert_eq!(
            omap::load_omap(&mut source, tree, 4096).unwrap().get(&200),
            Some(&expected)
        );
    }
    // The first value grows down from the end; xid=11 is the third value.
    let block = &mut source.get_mut()[8 * 4096..9 * 4096];
    let start = 4096 - 40 - 3 * 16;
    block[start..start + 4].copy_from_slice(&1u32.to_le_bytes());
    let sum = fletcher::fletcher64(&block[8..]);
    block[..8].copy_from_slice(&sum.to_le_bytes());
    let tree = omap::OmapTree { root: 8, xid: 11 };
    assert!(omap::omap_lookup(&mut source, tree, 4096, 200).is_err());
    assert!(
        !omap::load_omap(&mut source, tree, 4096)
            .unwrap()
            .contains_key(&200)
    );
}

#[test]
fn geometry_checksum_and_checkpoint_identity_are_validated() {
    for (offset, value) in [(36, 0u32), (36, u32::MAX), (180, 101)] {
        let mut bytes = BASIC.to_vec();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let checksum = fletcher::fletcher64(&bytes[8..4096]);
        bytes[..8].copy_from_slice(&checksum.to_le_bytes());
        assert!(superblock::read_nxsb(&mut Cursor::new(bytes)).is_err());
    }
    assert!(ApfsVolume::open(Cursor::new(&BASIC[..BASIC.len() - 1])).is_err());
    let mut bytes = BASIC.to_vec();
    bytes[4096 + 72] ^= 1;
    let checksum = fletcher::fletcher64(&bytes[4096 + 8..8192]);
    bytes[4096..4104].copy_from_slice(&checksum.to_le_bytes());
    assert_eq!(
        ApfsVolume::container_info(Cursor::new(bytes))
            .unwrap()
            .transaction_id,
        5
    );

    let mut bytes = BASIC.to_vec();
    bytes[36..40].copy_from_slice(&8192u32.to_le_bytes());
    bytes[40..48].copy_from_slice(&8u64.to_le_bytes());
    bytes[4096..8192].fill(0);
    let checksum = fletcher::fletcher64(&bytes[8..8192]);
    bytes[..8].copy_from_slice(&checksum.to_le_bytes());
    assert_eq!(
        superblock::read_nxsb(&mut Cursor::new(bytes))
            .unwrap()
            .block_size,
        8192
    );
}

#[test]
fn malformed_btree_offsets_and_counts_return_errors_without_panics() {
    let valid = &BASIC[10 * 4096..11 * 4096];
    for (offset, payload) in [
        (36, u32::MAX.to_le_bytes().to_vec()),
        (42, u16::MAX.to_le_bytes().to_vec()),
        (60, u16::MAX.to_le_bytes().to_vec()),
    ] {
        let mut block = valid.to_vec();
        block[offset..offset + payload.len()].copy_from_slice(&payload);
        if let Ok(node) = btree::BTreeNode::parse(&block) {
            assert!(node.value(0, 0).is_err());
        }
    }
}

#[test]
fn unicode_lookup_policy_preserves_case_and_canonical_equivalence() {
    let context = catalog::CatalogContext {
        incompatible_features: 9,
        ..catalog::CatalogContext::from(0)
    };
    assert_eq!(context.name_key("CAFÉ"), context.name_key("cafe\u{301}"));
    assert_eq!(context.name_key("Straße"), context.name_key("STRASSE"));
    let sensitive = catalog::CatalogContext {
        incompatible_features: 8,
        ..context
    };
    assert_ne!(
        sensitive.name_key("CAFÉ"),
        sensitive.name_key("cafe\u{301}")
    );
}

#[test]
fn snapshot_metadata_keeps_identity_times_and_superblock_provenance() {
    let mut volume = ApfsVolume::open(Cursor::new(BASIC)).unwrap();
    let snapshots = volume.list_snapshots().unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].name, "snap-one");
    assert_eq!(snapshots[0].transaction_id, 5);
    assert_eq!(snapshots[0].superblock_block, 4);
    assert_eq!(snapshots[0].create_time, -1);
    assert_eq!(snapshots[0].change_time, 123456789);
}

#[test]
fn file_extent_cannot_use_bytes_after_the_declared_container() {
    let mut source = BASIC.to_vec();
    source.extend_from_slice(&[b'X'; 8192]);
    let mut volume = ApfsVolume::open(Cursor::new(source)).unwrap();
    let records = vec![catalog::FileExtentRecord {
        logical_addr: 0,
        value: catalog::FileExtentVal {
            flags_and_length: 4096,
            phys_block_num: 16,
            crypto_id: 0,
        },
    }];
    assert!(extents::ApfsForkReader::new(&mut volume.reader, 4096, records, 4096).is_err());
}

#[test]
fn omap_predecessor_follows_the_correct_leaf_across_a_version_boundary() {
    fn node(level: u16, records: &[(u64, u64, u64)], root: bool) -> Vec<u8> {
        let mut block = vec![0u8; 4096];
        block[24..28].copy_from_slice(&object::OBJECT_TYPE_BTREE.to_le_bytes());
        let flags = btree::BTNODE_FIXED_KV_SIZE
            | if level == 0 { btree::BTNODE_LEAF } else { 0 }
            | if root { btree::BTNODE_ROOT } else { 0 };
        block[32..34].copy_from_slice(&flags.to_le_bytes());
        block[34..36].copy_from_slice(&level.to_le_bytes());
        block[36..40].copy_from_slice(&(records.len() as u32).to_le_bytes());
        block[42..44].copy_from_slice(&((records.len() * 4) as u16).to_le_bytes());
        let value_end = if root { 4056 } else { 4096 };
        let key_base = 56 + records.len() * 4;
        let value_size = if level == 0 { 16 } else { 8 };
        for (index, &(oid, xid, address)) in records.iter().enumerate() {
            block[56 + index * 4..58 + index * 4]
                .copy_from_slice(&((index * 16) as u16).to_le_bytes());
            block[58 + index * 4..60 + index * 4]
                .copy_from_slice(&(((index + 1) * value_size) as u16).to_le_bytes());
            let key = key_base + index * 16;
            block[key..key + 8].copy_from_slice(&oid.to_le_bytes());
            block[key + 8..key + 16].copy_from_slice(&xid.to_le_bytes());
            let value = value_end - (index + 1) * value_size;
            if level == 0 {
                block[value + 4..value + 8].copy_from_slice(&4096u32.to_le_bytes());
                block[value + 8..value + 16].copy_from_slice(&address.to_le_bytes());
            } else {
                block[value..value + 8].copy_from_slice(&address.to_le_bytes());
            }
        }
        if root {
            block[4060..4064].copy_from_slice(&4096u32.to_le_bytes());
            block[4064..4068].copy_from_slice(&16u32.to_le_bytes());
            block[4068..4072].copy_from_slice(&16u32.to_le_bytes());
        }
        let checksum = fletcher::fletcher64(&block[8..]);
        block[..8].copy_from_slice(&checksum.to_le_bytes());
        block
    }
    let mut source = Cursor::new(
        [
            node(1, &[(200, 5, 1), (200, 10, 2)], true),
            node(0, &[(200, 5, 9)], false),
            node(0, &[(200, 10, 10), (200, 11, 11)], false),
        ]
        .concat(),
    );
    for (xid, address) in [(5, 9), (9, 9), (10, 10), (11, 11)] {
        assert_eq!(
            omap::omap_lookup(&mut source, omap::OmapTree { root: 0, xid }, 4096, 200).unwrap(),
            address
        );
    }
}

#[test]
fn omap_allows_noheader_flag_and_rejects_encrypted() {
    fn omap_leaf(oid: u64, xid: u64, flags: u32, paddr: u64) -> Vec<u8> {
        let mut block = vec![0u8; 4096];
        block[24..28].copy_from_slice(&object::OBJECT_TYPE_BTREE.to_le_bytes());
        let bflags = btree::BTNODE_ROOT | btree::BTNODE_LEAF | btree::BTNODE_FIXED_KV_SIZE;
        block[32..34].copy_from_slice(&bflags.to_le_bytes());
        block[36..40].copy_from_slice(&1u32.to_le_bytes());
        block[42..44].copy_from_slice(&4u16.to_le_bytes());
        // TOC at 56: key_off=0, val_off=16
        block[56..58].copy_from_slice(&0u16.to_le_bytes());
        block[58..60].copy_from_slice(&16u16.to_le_bytes());
        // Key at 60: (oid, xid)
        block[60..68].copy_from_slice(&oid.to_le_bytes());
        block[68..76].copy_from_slice(&xid.to_le_bytes());
        // Val at 4056 - 16 = 4040
        block[4040..4044].copy_from_slice(&flags.to_le_bytes());
        block[4044..4048].copy_from_slice(&4096u32.to_le_bytes());
        block[4048..4056].copy_from_slice(&paddr.to_le_bytes());
        block[4060..4064].copy_from_slice(&4096u32.to_le_bytes());
        block[4064..4068].copy_from_slice(&16u32.to_le_bytes());
        block[4068..4072].copy_from_slice(&16u32.to_le_bytes());
        let checksum = fletcher::fletcher64(&block[8..]);
        block[..8].copy_from_slice(&checksum.to_le_bytes());
        block
    }

    let mut noheader_src = Cursor::new(omap_leaf(500, 10, omap::OMAP_VAL_NOHEADER, 42));
    let entry = omap::omap_lookup_entry(&mut noheader_src, omap::OmapTree::from(0), 4096, 500)
        .expect("OMAP_VAL_NOHEADER must be accepted");
    assert_eq!(entry, omap::OmapEntry { paddr: 42, noheader: true });

    let mut enc_src = Cursor::new(omap_leaf(500, 10, 0x04, 42));
    assert!(matches!(
        omap::omap_lookup_entry(&mut enc_src, omap::OmapTree::from(0), 4096, 500),
        Err(ApfsError::Unsupported(_))
    ));

    let mut crypto_src = Cursor::new(omap_leaf(500, 10, 0x10, 42));
    assert!(matches!(
        omap::omap_lookup_entry(&mut crypto_src, omap::OmapTree::from(0), 4096, 500),
        Err(ApfsError::Unsupported(_))
    ));
}

#[test]
fn sealed_hashed_noheader_btree_resolves_relative_child_oids() {
    // Blocks layout (4096B each):
    // 0: volume OMAP root (OID 1002 -> block 2 with OMAP_VAL_NOHEADER)
    // 1: catalog root (noheader, level 1, BTNODE_HASHED | BTNODE_NOHEADER, child relative OID=2)
    // 2: catalog leaf (noheader, level 0, inode key for OID=2)
    let mut omap_block = vec![0u8; 4096];
    omap_block[24..28].copy_from_slice(&object::OBJECT_TYPE_BTREE.to_le_bytes());
    let omap_flags = btree::BTNODE_ROOT | btree::BTNODE_LEAF | btree::BTNODE_FIXED_KV_SIZE;
    omap_block[32..34].copy_from_slice(&omap_flags.to_le_bytes());
    omap_block[36..40].copy_from_slice(&1u32.to_le_bytes());
    omap_block[42..44].copy_from_slice(&4u16.to_le_bytes());
    omap_block[56..58].copy_from_slice(&0u16.to_le_bytes());
    omap_block[58..60].copy_from_slice(&16u16.to_le_bytes());
    omap_block[60..68].copy_from_slice(&1002u64.to_le_bytes()); // root_tree_oid(1000) + rel(2)
    omap_block[68..76].copy_from_slice(&1u64.to_le_bytes());
    omap_block[4040..4044].copy_from_slice(&omap::OMAP_VAL_NOHEADER.to_le_bytes());
    omap_block[4044..4048].copy_from_slice(&4096u32.to_le_bytes());
    omap_block[4048..4056].copy_from_slice(&2u64.to_le_bytes());
    omap_block[4060..4064].copy_from_slice(&4096u32.to_le_bytes());
    omap_block[4064..4068].copy_from_slice(&16u32.to_le_bytes());
    omap_block[4068..4072].copy_from_slice(&16u32.to_le_bytes());
    let omap_sum = fletcher::fletcher64(&omap_block[8..]);
    omap_block[..8].copy_from_slice(&omap_sum.to_le_bytes());

    // Block 1: sealed hashed noheader internal root node (zeroed 32B obj_phys_t!)
    let mut root_block = vec![0u8; 4096];
    let rflags = btree::BTNODE_ROOT | btree::BTNODE_HASHED | btree::BTNODE_NOHEADER;
    root_block[32..34].copy_from_slice(&rflags.to_le_bytes());
    root_block[34..36].copy_from_slice(&1u16.to_le_bytes()); // level 1
    root_block[36..40].copy_from_slice(&1u32.to_le_bytes());
    root_block[42..44].copy_from_slice(&8u16.to_le_bytes());
    // Var TOC: key_off=0, key_len=8, val_off=40, val_len=40 (8B rel oid + 32B sha256)
    root_block[56..58].copy_from_slice(&0u16.to_le_bytes());
    root_block[58..60].copy_from_slice(&8u16.to_le_bytes());
    root_block[60..62].copy_from_slice(&40u16.to_le_bytes());
    root_block[62..64].copy_from_slice(&40u16.to_le_bytes());
    let min_key = (u64::from(catalog::J_TYPE_INODE) << 60) | 2u64;
    root_block[64..72].copy_from_slice(&min_key.to_le_bytes());
    let val_start = 4056 - 40;
    root_block[val_start..val_start + 8].copy_from_slice(&2u64.to_le_bytes()); // relative child OID
    root_block[val_start + 8..4056].fill(0xAA);
    root_block[4060..4064].copy_from_slice(&4096u32.to_le_bytes());

    // Block 2: sealed noheader leaf (zeroed 32B obj_phys_t!)
    let mut leaf_block = vec![0u8; 4096];
    let lflags = btree::BTNODE_LEAF | btree::BTNODE_NOHEADER;
    leaf_block[32..34].copy_from_slice(&lflags.to_le_bytes());
    leaf_block[34..36].copy_from_slice(&0u16.to_le_bytes());
    leaf_block[36..40].copy_from_slice(&1u32.to_le_bytes());
    leaf_block[42..44].copy_from_slice(&8u16.to_le_bytes());
    leaf_block[56..58].copy_from_slice(&0u16.to_le_bytes());
    leaf_block[58..60].copy_from_slice(&8u16.to_le_bytes());
    leaf_block[60..62].copy_from_slice(&4u16.to_le_bytes());
    leaf_block[62..64].copy_from_slice(&4u16.to_le_bytes());
    leaf_block[64..72].copy_from_slice(&min_key.to_le_bytes());
    leaf_block[4092..4096].copy_from_slice(&0x1234_5678u32.to_le_bytes());

    let mut image = Vec::new();
    image.extend_from_slice(&omap_block);
    image.extend_from_slice(&root_block);
    image.extend_from_slice(&leaf_block);
    let mut cursor = Cursor::new(image);

    let cmp = |key: &[u8]| {
        let (oid, jtype) = catalog::decode_catalog_key(key)?;
        Ok(catalog::compare_catalog_keys(
            oid,
            jtype,
            2,
            catalog::J_TYPE_INODE,
        ))
    };
    let opts = btree::BTreeReadOptions {
        root_noheader: true,
        sealed_base_oid: Some(1000),
    };
    let val = btree::btree_lookup_with_options(
        &mut cursor,
        1,
        4096,
        0,
        0,
        &cmp,
        Some(omap::OmapTree { root: 0, xid: 1 }),
        opts,
    )
    .expect("sealed relative lookup must succeed")
    .expect("entry must be found");
    assert_eq!(val, 0x1234_5678u32.to_le_bytes());

    let scanned = btree::btree_scan_with_options(
        &mut cursor,
        1,
        4096,
        0,
        0,
        &cmp,
        Some(omap::OmapTree { root: 0, xid: 1 }),
        opts,
    )
    .expect("sealed scan must succeed");
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0].1, 0x1234_5678u32.to_le_bytes());

    let cache = omap::load_omap_entries(&mut cursor, omap::OmapTree { root: 0, xid: 1 }, 4096)
        .expect("load_omap_entries");
    let mut walked = Vec::new();
    btree::btree_walk_leaves_with_entries(
        &mut cursor,
        1,
        4096,
        Some(&cache),
        opts,
        &mut |_, v| {
            walked.push(v.to_vec());
            true
        },
    )
    .expect("walk_leaves_with_entries must succeed");
    assert_eq!(walked, vec![0x1234_5678u32.to_le_bytes().to_vec()]);
}

#[test]
fn fext_tree_fallback_returns_sealed_file_extents() {
    // Block 0: empty valid catalog leaf (no J_TYPE_FILE_EXTENT records)
    let mut cat_block = vec![0u8; 4096];
    cat_block[24..28].copy_from_slice(&object::OBJECT_TYPE_BTREE.to_le_bytes());
    let cflags = btree::BTNODE_ROOT | btree::BTNODE_LEAF;
    cat_block[32..34].copy_from_slice(&cflags.to_le_bytes());
    cat_block[4060..4064].copy_from_slice(&4096u32.to_le_bytes());
    let cat_sum = fletcher::fletcher64(&cat_block[8..]);
    cat_block[..8].copy_from_slice(&cat_sum.to_le_bytes());

    // Block 1: physical fext_tree root leaf with 2 extents for private_id = 777
    let mut fext_block = vec![0u8; 4096];
    fext_block[24..28].copy_from_slice(&(object::OBJ_PHYSICAL | object::OBJECT_TYPE_BTREE).to_le_bytes());
    let fflags = btree::BTNODE_ROOT | btree::BTNODE_LEAF | btree::BTNODE_FIXED_KV_SIZE;
    fext_block[32..34].copy_from_slice(&fflags.to_le_bytes());
    fext_block[36..40].copy_from_slice(&2u32.to_le_bytes());
    fext_block[42..44].copy_from_slice(&8u16.to_le_bytes());
    // Entry 0: private_id=777, logical=0 -> len=4096, phys=50
    fext_block[56..58].copy_from_slice(&0u16.to_le_bytes());
    fext_block[58..60].copy_from_slice(&16u16.to_le_bytes());
    fext_block[64..72].copy_from_slice(&777u64.to_le_bytes());
    fext_block[72..80].copy_from_slice(&0u64.to_le_bytes());
    fext_block[4040..4048].copy_from_slice(&4096u64.to_le_bytes());
    fext_block[4048..4056].copy_from_slice(&50u64.to_le_bytes());
    // Entry 1: private_id=777, logical=4096 -> len=8192, phys=80
    fext_block[60..62].copy_from_slice(&16u16.to_le_bytes());
    fext_block[62..64].copy_from_slice(&32u16.to_le_bytes());
    fext_block[80..88].copy_from_slice(&777u64.to_le_bytes());
    fext_block[88..96].copy_from_slice(&4096u64.to_le_bytes());
    fext_block[4024..4032].copy_from_slice(&8192u64.to_le_bytes());
    fext_block[4032..4040].copy_from_slice(&80u64.to_le_bytes());
    fext_block[4060..4064].copy_from_slice(&4096u32.to_le_bytes());
    fext_block[4064..4068].copy_from_slice(&16u32.to_le_bytes());
    fext_block[4068..4072].copy_from_slice(&16u32.to_le_bytes());
    let fext_sum = fletcher::fletcher64(&fext_block[8..]);
    fext_block[..8].copy_from_slice(&fext_sum.to_le_bytes());

    let mut image = Vec::new();
    image.extend_from_slice(&cat_block);
    image.extend_from_slice(&fext_block);
    let mut cursor = Cursor::new(image);

    let ctx = catalog::CatalogContext {
        fext_tree_oid: 1,
        ..catalog::CatalogContext::from(0)
    };
    let extents = catalog::lookup_extents(&mut cursor, 0, ctx, 4096, 777).expect("fext lookup");
    assert_eq!(extents.len(), 2);
    assert_eq!(extents[0].logical_addr, 0);
    assert_eq!(extents[0].value.phys_block_num, 50);
    assert_eq!(extents[0].value.length(), 4096);
    assert_eq!(extents[1].logical_addr, 4096);
    assert_eq!(extents[1].value.phys_block_num, 80);
    assert_eq!(extents[1].value.length(), 8192);
}
