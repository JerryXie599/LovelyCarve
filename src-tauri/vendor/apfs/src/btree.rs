//! APFS B-tree traversal.
//!
//! Crate-private on purpose. Both entry points take a comparator that has to
//! reproduce the tree's on-disk key ordering, and violating that does not fail
//! loudly: the descent follows the wrong child, or the scan stops early, and
//! the caller gets `Ok(None)` or a short result set that is indistinguishable
//! from a genuine absence.
//!
//! APFS makes the ordering easy to get wrong. `obj_id_and_type` packs the
//! record type into the high nibble but sorts by OID first, so it is ordered
//! neither as the integer it looks like nor as the little-endian bytes it is
//! stored as. A caller reasoning about that for themselves is a caller who can
//! silently lose records.
//!
//! So the contract stays inside the crate, where it has a handful of call sites
//! that are all checked against real images. Code outside should use the
//! higher-level readers — `ApfsVolume`, or the `catalog`, `omap` and
//! `superblock` helpers — which own their comparators so callers do not have to
//! think about ordering at all.

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read, Seek};

use crate::error::{ApfsError, Result};
use crate::object::{self, ObjectHeader};
use crate::omap;

// B-tree node flags (from btn_flags)
pub const BTNODE_ROOT: u16 = 0x0001;
pub const BTNODE_LEAF: u16 = 0x0002;
pub const BTNODE_FIXED_KV_SIZE: u16 = 0x0004;
pub const BTNODE_HASHED: u16 = 0x0008;
pub const BTNODE_NOHEADER: u16 = 0x0010;

// BTreeInfo flags
#[expect(
    dead_code,
    reason = "mirrors the on-disk layout; parsed and kept so the struct documents the format"
)]
pub const BTREE_PHYSICAL: u32 = 0x0001;

/// B-tree node header — 56 bytes after the object header.
#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "mirrors the on-disk layout; parsed and kept so the struct documents the format"
)]
pub struct BTreeNodeHeader {
    pub btn_flags: u16,
    pub btn_level: u16,
    pub btn_nkeys: u32,
    pub btn_table_space_off: u16,
    pub btn_table_space_len: u16,
    pub btn_free_space_off: u16,
    pub btn_free_space_len: u16,
    pub btn_free_list_off: u16,
    pub btn_free_list_len: u16,
    pub btn_key_free_list_off: u16,
    pub btn_key_free_list_len: u16,
    pub btn_val_free_list_off: u16,
    pub btn_val_free_list_len: u16,
}

impl BTreeNodeHeader {
    pub const SIZE: usize = 24;

    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < Self::SIZE {
            return Err(ApfsError::InvalidBTree(
                "btree node header too short".into(),
            ));
        }
        let mut cursor = Cursor::new(data);
        Ok(BTreeNodeHeader {
            btn_flags: cursor.read_u16::<LittleEndian>()?,
            btn_level: cursor.read_u16::<LittleEndian>()?,
            btn_nkeys: cursor.read_u32::<LittleEndian>()?,
            btn_table_space_off: cursor.read_u16::<LittleEndian>()?,
            btn_table_space_len: cursor.read_u16::<LittleEndian>()?,
            btn_free_space_off: cursor.read_u16::<LittleEndian>()?,
            btn_free_space_len: cursor.read_u16::<LittleEndian>()?,
            btn_free_list_off: cursor.read_u16::<LittleEndian>()?,
            btn_free_list_len: cursor.read_u16::<LittleEndian>()?,
            btn_key_free_list_off: cursor.read_u16::<LittleEndian>()?,
            btn_key_free_list_len: cursor.read_u16::<LittleEndian>()?,
            btn_val_free_list_off: cursor.read_u16::<LittleEndian>()?,
            btn_val_free_list_len: cursor.read_u16::<LittleEndian>()?,
        })
    }

    pub fn is_leaf(&self) -> bool {
        self.btn_flags & BTNODE_LEAF != 0
    }

    pub fn is_root(&self) -> bool {
        self.btn_flags & BTNODE_ROOT != 0
    }

    pub fn is_fixed_kv(&self) -> bool {
        self.btn_flags & BTNODE_FIXED_KV_SIZE != 0
    }

    pub fn is_hashed(&self) -> bool {
        self.btn_flags & BTNODE_HASHED != 0
    }

    pub fn is_noheader(&self) -> bool {
        self.btn_flags & BTNODE_NOHEADER != 0
    }
}

/// BTreeInfo — 40 bytes at the end of a root node (before the footer).
#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "mirrors the on-disk layout; parsed and kept so the struct documents the format"
)]
pub struct BTreeInfo {
    pub bt_fixed: BTreeInfoFixed,
    pub bt_longest_key: u32,
    pub bt_longest_val: u32,
    pub bt_key_count: u64,
    pub bt_node_count: u64,
}

#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "mirrors the on-disk layout; parsed and kept so the struct documents the format"
)]
pub struct BTreeInfoFixed {
    pub bt_flags: u32,
    pub bt_node_size: u32,
    pub bt_key_size: u32,
    pub bt_val_size: u32,
}

impl BTreeInfo {
    pub const SIZE: usize = 40;

    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < Self::SIZE {
            return Err(ApfsError::InvalidBTree("btree info too short".into()));
        }
        let mut cursor = Cursor::new(data);
        let bt_flags = cursor.read_u32::<LittleEndian>()?;
        let bt_node_size = cursor.read_u32::<LittleEndian>()?;
        let bt_key_size = cursor.read_u32::<LittleEndian>()?;
        let bt_val_size = cursor.read_u32::<LittleEndian>()?;
        let bt_longest_key = cursor.read_u32::<LittleEndian>()?;
        let bt_longest_val = cursor.read_u32::<LittleEndian>()?;
        let bt_key_count = cursor.read_u64::<LittleEndian>()?;
        let bt_node_count = cursor.read_u64::<LittleEndian>()?;

        Ok(BTreeInfo {
            bt_fixed: BTreeInfoFixed {
                bt_flags,
                bt_node_size,
                bt_key_size,
                bt_val_size,
            },
            bt_longest_key,
            bt_longest_val,
            bt_key_count,
            bt_node_count,
        })
    }
}

/// A Table of Contents entry (fixed-size KV: 4 bytes, variable-size: 8 bytes)
#[derive(Debug, Clone)]
pub struct TocEntry {
    pub key_off: u16,
    pub key_len: u16, // 0 for fixed-size KV
    pub val_off: u16,
    pub val_len: u16, // 0 for fixed-size KV
}

/// A parsed APFS B-tree node with extracted key-value pairs.
pub struct BTreeNode {
    #[expect(
        dead_code,
        reason = "mirrors the on-disk layout; parsed and kept so the struct documents the format"
    )]
    pub header: ObjectHeader,
    pub node_header: BTreeNodeHeader,
    pub toc: Vec<TocEntry>,
    pub block_data: Vec<u8>,
    pub key_area_off: usize, // Absolute offset within block_data where key area starts
    pub val_area_end: usize, // Absolute offset within block_data where val area ends
    pub info: Option<BTreeInfo>,
}

impl BTreeNode {
    /// Parse a B-tree node from a raw block.
    pub fn parse(block: &[u8]) -> Result<Self> {
        Self::parse_node(block, false)
    }

    /// Parse a B-tree node, optionally allowing zeroed `obj_phys_t` headers
    /// on nodes flagged `BTNODE_NOHEADER` (OMAP_VAL_NOHEADER).
    pub fn parse_node(block: &[u8], allow_noheader: bool) -> Result<Self> {
        let header = ObjectHeader::parse(block)?;
        let node_header = BTreeNodeHeader::parse(&block[ObjectHeader::SIZE..])?;
        let header_zeroed = block[..ObjectHeader::SIZE].iter().all(|&b| b == 0);
        let valid_noheader = allow_noheader && header_zeroed && node_header.is_noheader();
        let valid_typed = matches!(
            header.object_type(),
            object::OBJECT_TYPE_BTREE | object::OBJECT_TYPE_BTREE_NODE
        );
        if (!valid_typed && !valid_noheader)
            || node_header.btn_level > 64
            || node_header.is_leaf() != (node_header.btn_level == 0)
        {
            return Err(ApfsError::InvalidBTree("invalid node type or level".into()));
        }

        let toc_start =
            ObjectHeader::SIZE + BTreeNodeHeader::SIZE + node_header.btn_table_space_off as usize;
        let fixed_kv = node_header.is_fixed_kv();

        // Key area starts right after the table space
        let key_area_off = ObjectHeader::SIZE
            + BTreeNodeHeader::SIZE
            + node_header.btn_table_space_off as usize
            + node_header.btn_table_space_len as usize;
        let toc_width = if fixed_kv { 4 } else { 8 };
        if key_area_off > block.len()
            || node_header.btn_nkeys as usize > node_header.btn_table_space_len as usize / toc_width
        {
            return Err(ApfsError::InvalidBTree("node table outside block".into()));
        }

        // Parse BTreeInfo if this is a root node (it's at the end of the value area)
        let info = if node_header.is_root() {
            let info_start = block
                .len()
                .checked_sub(BTreeInfo::SIZE)
                .ok_or_else(|| ApfsError::InvalidBTree("short root node".into()))?;
            Some(BTreeInfo::parse(&block[info_start..])?)
        } else {
            None
        };

        // Value area end: for root nodes, it's before BTreeInfo; for non-root, it's end of block
        let val_area_end = if node_header.is_root() {
            block.len() - BTreeInfo::SIZE
        } else {
            block.len()
        };

        // Parse TOC entries
        let mut toc = Vec::with_capacity(node_header.btn_nkeys as usize);
        let mut cursor = Cursor::new(&block[toc_start..]);

        for _ in 0..node_header.btn_nkeys {
            if fixed_kv {
                let key_off = cursor.read_u16::<LittleEndian>()?;
                let val_off = cursor.read_u16::<LittleEndian>()?;
                toc.push(TocEntry {
                    key_off,
                    key_len: 0,
                    val_off,
                    val_len: 0,
                });
            } else {
                let key_off = cursor.read_u16::<LittleEndian>()?;
                let key_len = cursor.read_u16::<LittleEndian>()?;
                let val_off = cursor.read_u16::<LittleEndian>()?;
                let val_len = cursor.read_u16::<LittleEndian>()?;
                toc.push(TocEntry {
                    key_off,
                    key_len,
                    val_off,
                    val_len,
                });
            }
        }

        Ok(BTreeNode {
            header,
            node_header,
            toc,
            block_data: block.to_vec(),
            key_area_off,
            val_area_end,
            info,
        })
    }

    /// Get the key bytes for a given TOC index.
    pub fn key(&self, index: usize, fixed_key_size: u32) -> Result<&[u8]> {
        let entry = self
            .toc
            .get(index)
            .ok_or_else(|| ApfsError::InvalidBTree("key index outside table".into()))?;
        let start = self.key_area_off + entry.key_off as usize;
        let len = if self.node_header.is_fixed_kv() {
            fixed_key_size as usize
        } else {
            entry.key_len as usize
        };
        let end = start + len;
        if end > self.val_area_end {
            return Err(ApfsError::InvalidBTree(format!(
                "key out of bounds: start={}, len={}, block_size={}",
                start,
                len,
                self.block_data.len()
            )));
        }
        Ok(&self.block_data[start..end])
    }

    /// Get the value bytes for a given TOC index.
    ///
    /// val_off is an offset from val_area_end to the START of the value data.
    /// i.e., value bytes are at block_data[val_area_end - val_off .. val_area_end - val_off + len].
    ///
    /// For internal (non-leaf) nodes, the value is always an oid_t (u64, 8 bytes).
    pub fn value(&self, index: usize, fixed_val_size: u32) -> Result<&[u8]> {
        let entry = self
            .toc
            .get(index)
            .ok_or_else(|| ApfsError::InvalidBTree("value index outside table".into()))?;
        let len = if !self.node_header.is_leaf() {
            // Non-leaf node values begin with child_oid: u64 (8 bytes).
            // In BTNODE_HASHED internal nodes (val_len = 40 = u64 + [u8; 32] SHA-256),
            // val_off points to the start of the 40-byte value at val_area_end - val_off,
            // so [start..start+8] directly extracts binv_child_oid.
            8
        } else if self.node_header.is_fixed_kv() {
            fixed_val_size as usize
        } else {
            entry.val_len as usize
        };

        let val_off = entry.val_off as usize;
        let start = self
            .val_area_end
            .checked_sub(val_off)
            .ok_or_else(|| ApfsError::InvalidBTree("value offset underflow".into()))?;
        let end = start + len;
        if end > self.val_area_end || start < self.key_area_off {
            return Err(ApfsError::InvalidBTree(format!(
                "value out of bounds: start={}, len={}, val_area_end={}, block_size={}",
                start,
                len,
                self.val_area_end,
                self.block_data.len()
            )));
        }
        Ok(&self.block_data[start..end])
    }

    /// For index nodes, get the child OID at a given index.
    /// The value for index nodes is always an oid_t (u64, 8 bytes).
    pub fn child_oid(&self, index: usize) -> Result<u64> {
        let val = self.value(index, 8)?;
        if val.len() < 8 {
            return Err(ApfsError::InvalidBTree("child oid too short".into()));
        }
        Ok(u64::from_le_bytes([
            val[0], val[1], val[2], val[3], val[4], val[5], val[6], val[7],
        ]))
    }
}

/// Traversal options for APFS B-trees, including Sealed System Volume nodes.
#[derive(Debug, Clone, Copy, Default)]
pub struct BTreeReadOptions {
    pub root_noheader: bool,
    pub sealed_base_oid: Option<u64>,
}

/// Tree-shape constants passed through recursive B-tree traversal.
struct BTreeParams {
    block_size: u32,
    fixed_key_size: u32,
    fixed_val_size: u32,
    omap_root: Option<omap::OmapTree>,
    sealed_base_oid: Option<u64>,
}

fn read_btree_node<R: Read + Seek>(
    reader: &mut R,
    block: u64,
    block_size: u32,
    noheader: bool,
) -> Result<BTreeNode> {
    if noheader {
        let data = object::read_block(reader, block, block_size)?;
        BTreeNode::parse_node(&data, true)
    } else {
        let (_, data) = object::read_object(reader, block, block_size)?;
        BTreeNode::parse(&data)
    }
}

fn effective_child_oid(
    node: &BTreeNode,
    child_oid: u64,
    sealed_base_oid: Option<u64>,
) -> Result<u64> {
    if node.node_header.is_hashed()
        && node.node_header.is_noheader()
        && let Some(base) = sealed_base_oid
    {
        return base.checked_add(child_oid).ok_or_else(|| {
            ApfsError::CorruptedData("sealed child OID addition overflow".into())
        });
    }
    Ok(child_oid)
}

/// Resolve a child OID to an OMAP entry (physical block + noheader flag).
/// If `omap_root` is None, the child OID is already a physical block number.
fn resolve_child_entry<R: Read + Seek>(
    reader: &mut R,
    node: &BTreeNode,
    child_oid: u64,
    params: &BTreeParams,
) -> Result<omap::OmapEntry> {
    let target_oid = effective_child_oid(node, child_oid, params.sealed_base_oid)?;
    match params.omap_root {
        Some(omap) => omap::omap_lookup_entry(reader, omap, params.block_size, target_oid),
        None => Ok(omap::OmapEntry {
            paddr: target_oid,
            noheader: false,
        }),
    }
}

/// Find the greatest key not exceeding a target in a physical fixed-size tree.
/// OMAP needs a predecessor of (OID, XID), rather than the first matching OID.
pub(crate) fn btree_predecessor<R: Read + Seek, F>(
    reader: &mut R,
    mut block: u64,
    block_size: u32,
    key_size: u32,
    value_size: u32,
    compare: F,
) -> Result<Option<(Vec<u8>, Vec<u8>)>>
where
    F: Fn(&[u8]) -> Result<std::cmp::Ordering>,
{
    let mut visited = std::collections::HashSet::new();
    for _ in 0..64 {
        if !visited.insert(block) {
            return Err(ApfsError::InvalidBTree("cyclic OMAP tree".into()));
        }
        let (_, data) = object::read_object(reader, block, block_size)?;
        let node = BTreeNode::parse(&data)?;
        let mut selected = None;
        for index in 0..node.node_header.btn_nkeys as usize {
            if compare(node.key(index, key_size)?)?.is_gt() {
                break;
            }
            selected = Some(index);
        }
        let Some(index) = selected else {
            return Ok(None);
        };
        if node.node_header.is_leaf() {
            return Ok(Some((
                node.key(index, key_size)?.to_vec(),
                node.value(index, value_size)?.to_vec(),
            )));
        }
        block = node.child_oid(index)?;
    }
    Err(ApfsError::InvalidBTree("OMAP depth exceeds 64".into()))
}

/// Look up a key in a B-tree.
///
/// `compare_fn` takes key bytes and returns Ordering of the node key relative to the search key:
/// - Less: node key < search key
/// - Equal: match
/// - Greater: node key > search key
///
/// `compare_fn` must reproduce the tree's on-disk ordering, and is not an
/// equality test. Internal nodes hold separator keys that never equal the
/// search key, so the descent picks the last separator ordering `Less` or
/// `Equal` and follows its child pointer; a predicate that only recognises
/// matches gives the descent no direction. Leaf scans stop at the first key
/// ordering `Greater`, on the assumption that the search key would already
/// have appeared. A comparator that disagrees with the on-disk order therefore
/// does not merely slow the lookup down — it returns `Ok(None)` for keys that
/// are present, with no error to distinguish that from a genuine miss.
///
/// `omap_root`: Some(block) for virtual B-trees (catalog), None for physical (OMAP).
///
/// Returns the raw value bytes if found.
fn build_tree_params(
    node: &BTreeNode,
    block_size: u32,
    fixed_key_size: u32,
    fixed_val_size: u32,
    omap_root: Option<omap::OmapTree>,
    sealed_base_oid: Option<u64>,
) -> BTreeParams {
    let (fks, fvs) = if let Some(ref info) = node.info {
        (
            if info.bt_fixed.bt_key_size > 0 {
                info.bt_fixed.bt_key_size
            } else {
                fixed_key_size
            },
            if info.bt_fixed.bt_val_size > 0 {
                info.bt_fixed.bt_val_size
            } else {
                fixed_val_size
            },
        )
    } else {
        (fixed_key_size, fixed_val_size)
    };
    BTreeParams {
        block_size,
        fixed_key_size: fks,
        fixed_val_size: fvs,
        omap_root,
        sealed_base_oid,
    }
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "preserved compatibility wrapper; catalog callers use btree_lookup_with_options"
    )
)]
pub fn btree_lookup<R: Read + Seek, F>(
    reader: &mut R,
    root_block: u64,
    block_size: u32,
    fixed_key_size: u32,
    fixed_val_size: u32,
    compare_fn: &F,
    omap_root: Option<omap::OmapTree>,
) -> Result<Option<Vec<u8>>>
where
    F: Fn(&[u8]) -> Result<std::cmp::Ordering>,
{
    btree_lookup_with_options(
        reader,
        root_block,
        block_size,
        fixed_key_size,
        fixed_val_size,
        compare_fn,
        omap_root,
        BTreeReadOptions::default(),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "internal helper extending btree_lookup with sealed-volume options"
)]
pub fn btree_lookup_with_options<R: Read + Seek, F>(
    reader: &mut R,
    root_block: u64,
    block_size: u32,
    fixed_key_size: u32,
    fixed_val_size: u32,
    compare_fn: &F,
    omap_root: Option<omap::OmapTree>,
    options: BTreeReadOptions,
) -> Result<Option<Vec<u8>>>
where
    F: Fn(&[u8]) -> Result<std::cmp::Ordering>,
{
    let node = read_btree_node(reader, root_block, block_size, options.root_noheader)?;
    let params = build_tree_params(
        &node,
        block_size,
        fixed_key_size,
        fixed_val_size,
        omap_root,
        options.sealed_base_oid,
    );
    btree_lookup_node(reader, &node, &params, compare_fn)
}

/// Debug-only check that `compare_fn` agrees with the node's on-disk key order.
///
/// Stored keys ascend, so comparing each of them against one fixed search key
/// must produce a non-decreasing run of `Less`, then `Equal`, then `Greater`.
/// An inversion proves the comparator orders keys differently than the tree
/// does, which makes both the leaf scan's early exit and the descent's choice
/// of child unsound: the lookup reports a miss for a key that is present, with
/// nothing to distinguish that from a genuine absence.
#[cfg(debug_assertions)]
fn debug_assert_comparator_matches_node_order<F>(
    node: &BTreeNode,
    params: &BTreeParams,
    compare_fn: &F,
) -> Result<()>
where
    F: Fn(&[u8]) -> Result<std::cmp::Ordering>,
{
    let mut prev: Option<(usize, std::cmp::Ordering)> = None;
    for i in 0..node.node_header.btn_nkeys as usize {
        let ord = compare_fn(node.key(i, params.fixed_key_size)?)?;
        if let Some((prev_i, prev_ord)) = prev {
            assert!(
                ord >= prev_ord,
                "b-tree comparator disagrees with on-disk key order: key {prev_i} orders \
                 {prev_ord:?} against the search key but key {i} orders {ord:?}"
            );
        }
        prev = Some((i, ord));
    }
    Ok(())
}

fn btree_lookup_node<R: Read + Seek, F>(
    reader: &mut R,
    node: &BTreeNode,
    params: &BTreeParams,
    compare_fn: &F,
) -> Result<Option<Vec<u8>>>
where
    F: Fn(&[u8]) -> Result<std::cmp::Ordering>,
{
    #[cfg(debug_assertions)]
    debug_assert_comparator_matches_node_order(node, params, compare_fn)?;

    if node.node_header.is_leaf() {
        // Search leaf for exact match
        for i in 0..node.node_header.btn_nkeys as usize {
            let key = node.key(i, params.fixed_key_size)?;
            match compare_fn(key)? {
                std::cmp::Ordering::Equal => {
                    let val = node.value(i, params.fixed_val_size)?;
                    return Ok(Some(val.to_vec()));
                }
                std::cmp::Ordering::Greater => return Ok(None),
                std::cmp::Ordering::Less => continue,
            }
        }
        Ok(None)
    } else {
        // Internal node: find the last key <= search key, follow child pointer
        let mut child_idx: Option<usize> = None;

        for i in 0..node.node_header.btn_nkeys as usize {
            let key = node.key(i, params.fixed_key_size)?;
            match compare_fn(key)? {
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal => {
                    child_idx = Some(i);
                }
                std::cmp::Ordering::Greater => break,
            }
        }

        let child_idx = match child_idx {
            Some(i) => i,
            None => return Ok(None),
        };

        let child_oid = node.child_oid(child_idx)?;
        let child_entry = resolve_child_entry(reader, node, child_oid, params)?;
        let child_node = read_btree_node(
            reader,
            child_entry.paddr,
            params.block_size,
            child_entry.noheader,
        )?;
        if child_node.node_header.btn_level + 1 != node.node_header.btn_level {
            return Err(ApfsError::InvalidBTree(
                "child level does not decrease (cycle or malformed tree)".into(),
            ));
        }

        btree_lookup_node(reader, &child_node, params, compare_fn)
    }
}

#[derive(Clone, Copy)]
enum OmapCacheRef<'a> {
    Physical,
    Paddr(&'a std::collections::HashMap<u64, u64>),
    Entries(&'a std::collections::HashMap<u64, omap::OmapEntry>),
}

/// Stream every leaf entry of a B-tree in key order through `visit`.
///
/// Unlike [`btree_scan`] this never materialises the result set: each
/// `(key, value)` pair is handed to the visitor and dropped, so a whole-tree
/// walk over a multi-million-record catalog stays at one block of working
/// memory. Child OIDs are resolved through `omap_cache` — a preloaded
/// oid→paddr map (see `omap::load_omap`) — instead of a per-child OMAP
/// B-tree descent, which is what makes a full catalog walk O(tree nodes)
/// block reads rather than O(tree nodes × OMAP depth). `None` means the
/// tree is physical (child OIDs are already block numbers).
///
/// The visitor returns `false` to stop early (cancellation); the walk then
/// returns `Ok(false)` without an error.
pub fn btree_walk_leaves<R: Read + Seek>(
    reader: &mut R,
    root_block: u64,
    block_size: u32,
    omap_cache: Option<&std::collections::HashMap<u64, u64>>,
    visit: &mut dyn FnMut(&[u8], &[u8]) -> bool,
) -> Result<bool> {
    let cache = match omap_cache {
        Some(map) => OmapCacheRef::Paddr(map),
        None => OmapCacheRef::Physical,
    };
    btree_walk_leaves_inner(
        reader,
        root_block,
        block_size,
        cache,
        BTreeReadOptions::default(),
        visit,
    )
}

/// Stream leaf entries using full `OmapEntry` metadata and Sealed System Volume options.
pub fn btree_walk_leaves_with_entries<R: Read + Seek>(
    reader: &mut R,
    root_block: u64,
    block_size: u32,
    omap_cache: Option<&std::collections::HashMap<u64, omap::OmapEntry>>,
    options: BTreeReadOptions,
    visit: &mut dyn FnMut(&[u8], &[u8]) -> bool,
) -> Result<bool> {
    let cache = match omap_cache {
        Some(map) => OmapCacheRef::Entries(map),
        None => OmapCacheRef::Physical,
    };
    btree_walk_leaves_inner(reader, root_block, block_size, cache, options, visit)
}

fn btree_walk_leaves_inner<R: Read + Seek>(
    reader: &mut R,
    root_block: u64,
    block_size: u32,
    omap_cache: OmapCacheRef<'_>,
    options: BTreeReadOptions,
    visit: &mut dyn FnMut(&[u8], &[u8]) -> bool,
) -> Result<bool> {
    let node = read_btree_node(reader, root_block, block_size, options.root_noheader)?;
    let params = build_tree_params(&node, block_size, 0, 0, None, options.sealed_base_oid);
    btree_walk_node(reader, &node, &params, omap_cache, visit)
}

fn btree_walk_node<R: Read + Seek>(
    reader: &mut R,
    node: &BTreeNode,
    params: &BTreeParams,
    omap_cache: OmapCacheRef<'_>,
    visit: &mut dyn FnMut(&[u8], &[u8]) -> bool,
) -> Result<bool> {
    if node.node_header.is_leaf() {
        for i in 0..node.node_header.btn_nkeys as usize {
            let key = node.key(i, params.fixed_key_size)?;
            let val = node.value(i, params.fixed_val_size)?;
            if !visit(key, val) {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    for i in 0..node.node_header.btn_nkeys as usize {
        let child_oid = node.child_oid(i)?;
        let target_oid = effective_child_oid(node, child_oid, params.sealed_base_oid)?;
        let child_entry = match omap_cache {
            OmapCacheRef::Entries(map) => *map.get(&target_oid).ok_or_else(|| {
                ApfsError::CorruptedData(format!("OMAP cache: OID {target_oid} not found"))
            })?,
            OmapCacheRef::Paddr(map) => omap::OmapEntry {
                paddr: *map.get(&target_oid).ok_or_else(|| {
                    ApfsError::CorruptedData(format!("OMAP cache: OID {target_oid} not found"))
                })?,
                noheader: false,
            },
            OmapCacheRef::Physical => omap::OmapEntry {
                paddr: target_oid,
                noheader: false,
            },
        };
        let child_node = read_btree_node(
            reader,
            child_entry.paddr,
            params.block_size,
            child_entry.noheader,
        )?;
        if child_node.node_header.btn_level + 1 != node.node_header.btn_level {
            return Err(ApfsError::InvalidBTree(
                "child level does not decrease (cycle or malformed tree)".into(),
            ));
        }
        if !btree_walk_node(reader, &child_node, params, omap_cache, visit)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Scan a B-tree, collecting every key-value pair whose key orders `Equal`.
///
/// `compare_fn` is the same comparator [`btree_lookup`] takes, and carries the
/// same contract: it must reproduce the tree's on-disk ordering. The scan reads
/// it as a range — `Less` is before the range, `Equal` is in it, and `Greater`
/// ends the scan — so a comparator that disagrees with the on-disk order
/// silently truncates the results.
///
/// `omap_root`: Some(block) for virtual B-trees, None for physical.
pub fn btree_scan<R: Read + Seek, F>(
    reader: &mut R,
    root_block: u64,
    block_size: u32,
    fixed_key_size: u32,
    fixed_val_size: u32,
    compare_fn: &F,
    omap_root: Option<omap::OmapTree>,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>>
where
    F: Fn(&[u8]) -> Result<std::cmp::Ordering>,
{
    btree_scan_with_options(
        reader,
        root_block,
        block_size,
        fixed_key_size,
        fixed_val_size,
        compare_fn,
        omap_root,
        BTreeReadOptions::default(),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "internal helper extending btree_scan with sealed-volume options"
)]
pub fn btree_scan_with_options<R: Read + Seek, F>(
    reader: &mut R,
    root_block: u64,
    block_size: u32,
    fixed_key_size: u32,
    fixed_val_size: u32,
    compare_fn: &F,
    omap_root: Option<omap::OmapTree>,
    options: BTreeReadOptions,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>>
where
    F: Fn(&[u8]) -> Result<std::cmp::Ordering>,
{
    let node = read_btree_node(reader, root_block, block_size, options.root_noheader)?;
    let params = build_tree_params(
        &node,
        block_size,
        fixed_key_size,
        fixed_val_size,
        omap_root,
        options.sealed_base_oid,
    );
    let mut results = Vec::new();
    btree_scan_node(reader, &node, &params, compare_fn, &mut results)?;
    Ok(results)
}

fn btree_scan_node<R: Read + Seek, F>(
    reader: &mut R,
    node: &BTreeNode,
    params: &BTreeParams,
    compare_fn: &F,
    results: &mut Vec<(Vec<u8>, Vec<u8>)>,
) -> Result<bool>
// returns false if scanning should stop
where
    F: Fn(&[u8]) -> Result<std::cmp::Ordering>,
{
    if node.node_header.is_leaf() {
        #[cfg(debug_assertions)]
        debug_assert_comparator_matches_node_order(node, params, compare_fn)?;

        for i in 0..node.node_header.btn_nkeys as usize {
            let key = node.key(i, params.fixed_key_size)?;
            match compare_fn(key)? {
                std::cmp::Ordering::Equal => {
                    let val = node.value(i, params.fixed_val_size)?;
                    results.push((key.to_vec(), val.to_vec()));
                }
                std::cmp::Ordering::Less => continue,
                std::cmp::Ordering::Greater => return Ok(false),
            }
        }
        Ok(true)
    } else {
        let nkeys = node.node_header.btn_nkeys as usize;
        for i in 0..nkeys {
            let key = node.key(i, params.fixed_key_size)?;
            if compare_fn(key)?.is_gt() {
                return Ok(false);
            }
            if i + 1 < nkeys && compare_fn(node.key(i + 1, params.fixed_key_size)?)?.is_lt() {
                continue;
            }
            let child_oid = node.child_oid(i)?;
            let child_entry = resolve_child_entry(reader, node, child_oid, params)?;
            let child_node = read_btree_node(
                reader,
                child_entry.paddr,
                params.block_size,
                child_entry.noheader,
            )?;
            if child_node.node_header.btn_level + 1 != node.node_header.btn_level {
                return Err(ApfsError::InvalidBTree(
                    "child level does not decrease (cycle or malformed tree)".into(),
                ));
            }

            if !btree_scan_node(reader, &child_node, params, compare_fn, results)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
