use super::{header::*, layout::*, metadata::*, test_support::*, text::*};
use crate::core::evidence::partitions::{test_support::BufSource, SectorSource};
use std::io::{Read, Seek, SeekFrom};

#[test]
fn lvm_labels_all_four_sectors_and_wrapped_committed_metadata() {
    for sector in 0..4 {
        for wrap in [false, true] {
            let bytes = image(&text(), sector, wrap);
            let source = BufSource(bytes.clone());
            let metadata = read_metadata(&source, 0, source.size()).unwrap();
            assert_eq!(metadata.header.label_offset, sector as u64 * 512);
            assert_eq!(metadata.volume_group.sequence_number, 7);
            assert_eq!(metadata.text_ranges.len(), if wrap { 2 } else { 1 });
            assert_eq!(locate_active_metadata_region(&bytes).is_some(), !wrap);
        }
    }
}

#[test]
fn lvm_ignores_higher_sequence_historical_and_precommitted_text() {
    let mut bytes = image(&text(), 1, false);
    let decoy = text().replace("seqno = 7", "seqno = 999");
    let offset = MDA_OFFSET + 8192;
    bytes[offset..offset + decoy.len()].copy_from_slice(decoy.as_bytes());
    // A precommitted raw_locns[1] points at the decoy. raw_locns[0] remains authoritative.
    bytes[MDA_OFFSET + 64..MDA_OFFSET + 72].copy_from_slice(&8192u64.to_le_bytes());
    bytes[MDA_OFFSET + 72..MDA_OFFSET + 80].copy_from_slice(&(decoy.len() as u64).to_le_bytes());
    bytes[MDA_OFFSET + 80..MDA_OFFSET + 84]
        .copy_from_slice(&checksum(decoy.as_bytes()).to_le_bytes());
    refresh_mda_checksum(&mut bytes, MDA_OFFSET);
    let source = BufSource(bytes);
    assert_eq!(
        read_metadata(&source, 0, source.size())
            .unwrap()
            .volume_group
            .sequence_number,
        7
    );
}

#[test]
fn lvm_crc_corruption_and_ignored_locations_never_fall_back_to_old_text() {
    for offset in [512 + 16, MDA_OFFSET, MDA_OFFSET + 512] {
        let mut bytes = image(&text(), 1, false);
        bytes[offset] ^= 1;
        let source = BufSource(bytes);
        assert!(read_metadata(&source, 0, source.size()).is_err());
    }
    for flags in [1u32, 2] {
        let mut bytes = image(&text(), 1, false);
        bytes[MDA_OFFSET + 60..MDA_OFFSET + 64].copy_from_slice(&flags.to_le_bytes());
        refresh_mda_checksum(&mut bytes, MDA_OFFSET);
        let source = BufSource(bytes);
        assert!(read_metadata(&source, 0, source.size()).is_err());
    }
}

#[test]
fn lvm_backup_mda_outside_head_probe_and_conflicting_copies() {
    let mut bytes = image(&text(), 1, false);
    let backup = 8 * 1024 * 1024;
    bytes.resize(backup + MDA_SIZE, 0);
    put_label(&mut bytes, 1, &[(MDA_OFFSET, MDA_SIZE), (backup, MDA_SIZE)]);
    let newer = text().replace("seqno = 7", "seqno = 8");
    put_metadata(&mut bytes, backup, MDA_SIZE, 512, newer.as_bytes());
    let source = BufSource(bytes.clone());
    assert_eq!(
        read_metadata(&source, 0, source.size())
            .unwrap()
            .volume_group
            .sequence_number,
        8
    );
    bytes[MDA_OFFSET] ^= 1;
    let source = BufSource(bytes.clone());
    let metadata = read_metadata(&source, 0, source.size()).unwrap();
    assert_eq!(metadata.volume_group.sequence_number, 8);
    assert_eq!(metadata.diagnostics.len(), 1);
    put_metadata(
        &mut bytes,
        MDA_OFFSET,
        MDA_SIZE,
        512,
        newer.replace("/dev/evidence", "/dev/conflict").as_bytes(),
    );
    let source = BufSource(bytes);
    assert!(read_metadata(&source, 0, source.size()).is_err());
}

#[test]
fn lvm_text_grammar_comments_strings_duplicate_fields_and_empty_vg() {
    let metadata = parse_volume_group(text().as_bytes()).unwrap();
    assert_eq!(metadata.logical_volumes[0].segments[0].start_extent, 0);
    assert_eq!(
        metadata.physical_volumes[1].device.as_deref(),
        Some("/dev/evidence")
    );
    for bad in [
        text().replace("extent_size = 8", "extent_size = 8 extent_size = 9"),
        text().replace("segment_count = 2", "segment_count = 3"),
        text().replace("start_extent = 2", "start_extent = 3"),
        text().replace("start_extent = 2", "start_extent = 1"),
        text().replace("stripe_count = 1", "stripe_count = 2"),
        text().replace("extent_size = 8", "extent_size = -1"),
        text().replace("extent_size = 8", "extent_size = 0"),
        text().replace("extent_size = 8", "extent_size = 18446744073709551616"),
    ] {
        assert!(parse_volume_group(bad.as_bytes()).is_err());
    }
    let empty = format!(
        r#"vg {{ id="BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB" seqno=1 extent_size=8 physical_volumes {{ pv0 {{ id="{PV_ID}" dev_size=256 pe_start=128 pe_count=16 }} }} }}"#
    );
    assert!(parse_volume_group(empty.as_bytes())
        .unwrap()
        .logical_volumes
        .is_empty());
}

#[test]
fn lvm_uuid_matching_same_size_pvs_and_fragmented_reads_agree() {
    let bytes = image(&text(), 1, false);
    let source = BufSource(bytes);
    let metadata = read_metadata(&source, 0, source.size()).unwrap();
    let group = &metadata.volume_group;
    let pv = &group.physical_volumes[1];
    let layout = linear_layout(group, &group.logical_volumes[0], pv, 0, source.size()).unwrap();
    assert!(layout.contiguous_offset().is_none());
    let expected = [vec![0x11; 8192], vec![0x22; 8192]].concat();
    assert_eq!(layout.read(&source, 0, usize::MAX).unwrap(), expected);
    let streamed: Vec<_> = (0..layout.size)
        .step_by(1703)
        .flat_map(|offset| layout.read(&source, offset, 1703).unwrap())
        .collect();
    assert_eq!(streamed, expected);
    for (offset, length) in [(0, 0), (8190, 4), (8192, 7000), (16000, 1000), (16384, 1)] {
        assert_eq!(
            layout.read(&source, offset, length).unwrap(),
            expected[offset as usize..(offset as usize + length).min(expected.len())]
        );
    }
    assert!(lv_regions_from_source(&source, 0, source.size()).is_empty());
    assert!(linear_layout(
        group,
        &group.logical_volumes[0],
        &group.physical_volumes[0],
        0,
        source.size()
    )
    .is_err());
    let mut broken = layout.clone();
    broken.segments[1].physical_offset = source.size() - 1;
    assert!(broken.read(&source, 8192, 2).is_err());
    assert!(layout.ranges(u64::MAX, 1).is_err());
}

#[test]
fn lvm_bounds_unsupported_types_and_non_512_disk_geometry() {
    for bad in [
        text().replace("pe_count = 16", "pe_count = 100000"),
        text().replace("pe_start = 128", "pe_start = 127"),
        text().replace(PV_ID, "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE"),
    ] {
        let source = BufSource(image(&bad, 1, false));
        assert!(read_metadata(&source, 0, source.size()).is_err());
    }
    let source = BufSource(image(
        &text().replace("type = \"striped\"", "type = \"snapshot\""),
        1,
        false,
    ));
    let metadata = read_metadata(&source, 0, source.size()).unwrap();
    let group = &metadata.volume_group;
    assert!(linear_layout(
        group,
        &group.logical_volumes[0],
        &group.physical_volumes[1],
        0,
        source.size()
    )
    .is_err());
    let mut bytes = vec![0; 4096];
    bytes.extend(image(&text(), 1, false));
    let source = BufSource(bytes);
    let metadata = read_metadata(&source, 4096, source.size() - 4096).unwrap();
    let group = &metadata.volume_group;
    let layout = linear_layout(
        group,
        &group.logical_volumes[0],
        &group.physical_volumes[1],
        4096,
        source.size() - 4096,
    )
    .unwrap();
    assert_eq!(
        layout.segments[0].physical_offset,
        4096 + PE_START as u64 + 12 * 4096
    );
}

#[test]
fn lvm_seek_and_read_at_have_independent_positions_and_checked_bounds() {
    assert_eq!(checksum(b"123456789"), 0x4991_cf02);
    let source = BufSource(image(&text(), 1, false));
    let metadata = read_metadata(&source, 0, source.size()).unwrap();
    let group = &metadata.volume_group;
    let layout = linear_layout(
        group,
        &group.logical_volumes[0],
        &group.physical_volumes[1],
        0,
        source.size(),
    )
    .unwrap();
    let mut reader = LogicalVolumeReader::new(&source, &layout);
    assert_eq!(reader.seek(SeekFrom::Start(8190)).unwrap(), 8190);
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes, [0x11, 0x11, 0x22, 0x22]);
    assert_eq!(reader.read_at(0, 1).unwrap(), [0x11]);
    assert_eq!(reader.position(), 8194);
    assert!(reader.seek(SeekFrom::End(-16385)).is_err());
    assert_eq!(reader.position(), 8194);
    reader.seek(SeekFrom::Start(u64::MAX)).unwrap();
    assert!(reader.seek(SeekFrom::Current(1)).is_err());
    assert_eq!(reader.read(&mut bytes).unwrap(), 0);
}

#[test]
fn lvm_metadata_limits_uuid_identity_and_declaration_order() {
    assert!(parse_volume_group(text().replace(OTHER_PV_ID, PV_ID).as_bytes()).is_err());
    let depth = format!("{}{}", "nested { ".repeat(34), " }".repeat(34));
    assert!(parse_volume_group(depth.as_bytes()).is_err());
    let escaped = text().replace("/dev/evidence", r#"/dev/\"quoted\"\\path"#);
    assert_eq!(
        parse_volume_group(escaped.as_bytes())
            .unwrap()
            .physical_volumes[1]
            .device
            .as_deref(),
        Some("/dev/\"quoted\"\\path")
    );
    let ordered = text().replace("root {", "zulu {");
    let (first, last) = ordered.rsplit_once("    }\n}").unwrap();
    let ordered = format!(
        r#"{first}
        alpha {{ id="EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE" segment_count=1 segment1 {{ start_extent=0 extent_count=1 type="striped" stripe_count=1 stripes=["pv1",4] }} }}
    }}
}}{last}"#
    );
    let group = parse_volume_group(ordered.as_bytes()).unwrap();
    assert!(parse_volume_group(
        ordered
            .replace(
                "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE",
                "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC"
            )
            .as_bytes()
    )
    .is_err());
    assert_eq!(
        group
            .logical_volumes
            .iter()
            .map(|volume| volume.name.as_str())
            .collect::<Vec<_>>(),
        ["zulu", "alpha"]
    );
    let source = BufSource(image(&text(), 1, false));
    let metadata = read_metadata(&source, 0, source.size()).unwrap();
    let group = &metadata.volume_group;
    let mut overlapping = group.logical_volumes[0].clone();
    overlapping.segments[1].stripes[0].physical_extent = 13;
    assert!(linear_layout(
        group,
        &overlapping,
        &group.physical_volumes[1],
        0,
        source.size()
    )
    .is_err());
    assert!(linear_layout(
        group,
        &group.logical_volumes[0],
        &group.physical_volumes[1],
        u64::MAX,
        source.size()
    )
    .is_err());
}

#[test]
fn lvm_binary_range_version_truncation_and_probe_crc_are_checked() {
    for (field, value) in [
        (20, 2u64),
        (24, 4097),
        (32, MDA_SIZE as u64 + 1),
        (40, 511),
        (48, u64::MAX),
    ] {
        let mut bytes = image(&text(), 1, false);
        if field == 20 {
            bytes[MDA_OFFSET + field..MDA_OFFSET + field + 4]
                .copy_from_slice(&(value as u32).to_le_bytes());
        } else {
            bytes[MDA_OFFSET + field..MDA_OFFSET + field + 8].copy_from_slice(&value.to_le_bytes());
        }
        refresh_mda_checksum(&mut bytes, MDA_OFFSET);
        let source = BufSource(bytes);
        assert!(read_metadata(&source, 0, source.size()).is_err());
    }
    let mut bytes = image(&text(), 1, false);
    bytes[528] ^= 1;
    let source = BufSource(bytes);
    assert!(
        crate::core::evidence::partitions::detect_partitions(&source, 512)
            .unwrap()
            .is_empty()
    );
    let bytes = image(&text(), 1, false);
    struct Truncated(Vec<u8>);
    impl SectorSource for Truncated {
        fn size(&self) -> u64 {
            128 * 1024
        }
        fn read(&self, offset: u64, length: usize) -> anyhow::Result<Vec<u8>> {
            let from = (offset as usize).min(self.0.len());
            Ok(self.0[from..(from + length).min(self.0.len())].to_vec())
        }
    }
    assert!(read_metadata(
        &Truncated(bytes[..MDA_OFFSET + 520].to_vec()),
        0,
        128 * 1024
    )
    .is_err());
}

#[test]
fn lvm_accepts_shrunk_pv_and_data_area_dev_size_within_partition() {
    // Reproduces shrunk LVM2 PV geometry (e.g. `dev_size = partition_sectors - pe_start`,
    // `pe_count * extent_size <= dev_size`, binary PV size > partition_size).
    let partition_size = 128u64 * 1024;
    let data_sectors = (partition_size - PE_START as u64) / 512; // 128 sectors (64 KiB)
    let shrunk_text = text()
        .replace("dev_size = 256", &format!("dev_size = {data_sectors}"))
        .replace("pe_count = 16", "pe_count = 8") // 8 * 4096 = 32 KiB <= 64 KiB
        .replace(
            "extent_count = 2 type = \"striped\" stripe_count = 1 stripes = [ \"pv1\", 12 ]",
            "extent_count = 2 type = \"striped\" stripe_count = 1 stripes = [ \"pv1\", 4 ]",
        );
    let mut bytes = image(&shrunk_text, 1, false);
    // Marker bytes inside stripes at PE 0 and PE 4 to verify reads resolve accurately.
    bytes[PE_START..PE_START + 4].copy_from_slice(b"LV0!");
    bytes[PE_START + 4 * 4096..PE_START + 4 * 4096 + 4].copy_from_slice(b"LV1!");
    // Simulate original unshrunk PV size > partition_size in label sector (sector 1 = offset 512;
    // PV header starts at +32 so size_bytes is at label+64..72, data_area[0].size at label+80..88).
    bytes[512 + 64..512 + 72].copy_from_slice(&(256u64 * 1024).to_le_bytes());
    bytes[512 + 80..512 + 88].copy_from_slice(&0u64.to_le_bytes());
    let label_crc = checksum(&bytes[512 + 20..1024]);
    bytes[512 + 16..512 + 20].copy_from_slice(&label_crc.to_le_bytes());

    let source = BufSource(bytes);
    let metadata = read_metadata(&source, 0, source.size()).unwrap();
    let group = &metadata.volume_group;
    let layout = linear_layout(
        group,
        &group.logical_volumes[0],
        &group.physical_volumes[1],
        0,
        source.size(),
    )
    .unwrap();
    assert_eq!(layout.read(&source, 0, 4).unwrap(), b"LV1!");
    assert_eq!(layout.read(&source, 8192, 4).unwrap(), b"LV0!");

    // Negative 1: pe_start + pe_count * extent_size > partition_size must still fail.
    let overflow_text = shrunk_text.replace("pe_count = 8", "pe_count = 17");
    let source = BufSource(image(&overflow_text, 1, false));
    assert!(read_metadata(&source, 0, source.size()).is_err());

    // Negative 2: pe_count * extent_size > dev_size * 512 must still fail even inside partition.
    let small_dev_text = text()
        .replace("dev_size = 256", "dev_size = 64") // 32 KiB dev_size < 64 KiB (16 * 4096)
        .replace("pe_count = 16", "pe_count = 16");
    let small_dev_source = BufSource(image(&small_dev_text, 1, false));
    assert!(read_metadata(&small_dev_source, 0, small_dev_source.size()).is_err());
    let group = crate::core::evidence::partitions::lvm2::text::parse_volume_group(
        small_dev_text.as_bytes(),
    )
    .unwrap();
    assert!(linear_layout(
        &group,
        &group.logical_volumes[0],
        &group.physical_volumes[1],
        0,
        small_dev_source.size(),
    )
    .is_err());

    // Negative 3: metadata area extending past partition_size must fail even when PV size clamped.
    let mut bad_mda = vec![0u8; 128 * 1024];
    put_label(&mut bad_mda, 1, &[(MDA_OFFSET, 256 * 1024)]);
    bad_mda[512 + 64..512 + 72].copy_from_slice(&(512u64 * 1024).to_le_bytes());
    let label_crc = checksum(&bad_mda[512 + 20..1024]);
    bad_mda[512 + 16..512 + 20].copy_from_slice(&label_crc.to_le_bytes());
    assert!(parse_label(&bad_mda[512..1024], 512, bad_mda.len() as u64).is_err());
}
