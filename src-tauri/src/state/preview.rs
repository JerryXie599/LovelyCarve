//! Runtime preview state.
use std::sync::Arc;

use crate::core::common::ids::NodeRef;

/// Cached bytes of one inner ZIP archive (see [`AppState::zip_session`]).
/// `truncated` records whether the read hit the structure-parse cap, so the
/// preview can warn without re-stat-ing the file on a cache hit.
pub struct ZipBytesCache {
    pub key: NodeRef,
    pub bytes: Arc<Vec<u8>>,
    pub truncated: bool,
}

/// Cached bytes of one Realm database (see [`AppState::realm_session`]).
pub struct RealmBytesCache {
    pub key: NodeRef,
    pub bytes: Arc<[u8]>,
    pub file_size: u64,
}

/// Cached bytes and parsed records of one EVTX event log (see [`AppState::evtx_session`]).
pub struct EvtxBytesCache {
    pub key: String,
    pub bytes: Arc<[u8]>,
    pub file_size: u64,
    pub digest: crate::core::preview::evtx_preview::EvtxDigest,
    pub records: Vec<crate::core::preview::evtx_preview::EvtxRecordItem>,
}

/// Cached parser and digest of one Windows Registry Hive (see [`AppState::registry_session`]).
pub struct RegistrySessionCache {
    pub key: String,
    pub parser: notatin::parser::Parser,
    pub digest: crate::core::preview::registry_preview::RegistryDigest,
}

pub use crate::core::preview::mft_preview::MftSessionCache;
