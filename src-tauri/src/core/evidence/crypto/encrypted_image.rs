//! Session-only plaintext views over bounded, read-only evidence sources.
//!
//! BitLocker, LUKS, VeraCrypt and FileVault share this one unlock path: the
//! scheme opens its volume from a region of the evidence, and the plaintext
//! becomes a derived image that the caller attaches to the source partition
//! or keeps as evidence of its own.

use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use anyhow::{anyhow, bail, ensure, Context};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};

use crate::core::common::error::Failure;
use crate::core::evidence::crypto::bitlocker;
pub use crate::core::evidence::crypto::credential::{EncryptionScheme, UnlockCredential};
use crate::core::evidence::crypto::veracrypt_volume;
pub use crate::core::evidence::crypto::veracrypt_volume::{
    VeracryptDetails, VeracryptHeader, VeracryptPrf,
};
use crate::core::evidence::image::{DerivedImage, DiskImage, ImageInfo};
use crate::core::evidence::partitions::volume_probe::probe_volume;
use crate::core::evidence::partitions::{
    detect_filesystem, detect_partitions_with_meta, Partition, PartitionKind, SectorSource,
};
use crate::core::filesystem::{evidence_session::EvidenceSession, tree::fs_tree_read};

/// Sector size of every plaintext image this module builds.
const PLAINTEXT_SECTOR: usize = 512;

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UnlockTarget {
    #[default]
    NewImage,
    SourcePartition,
}

// Intentionally no Debug or Serialize: credentials must not enter diagnostics.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnlockOptions {
    #[serde(default)]
    pub target: UnlockTarget,
    pub scheme: EncryptionScheme,
    pub credential: UnlockCredential,
    pub partition_id: Option<u8>,
    #[serde(default)]
    pub node_id: Option<String>,
    pub offset: Option<u64>,
    pub length: Option<u64>,
    /// VeraCrypt: which headers to read.
    #[serde(default)]
    pub header: VeracryptHeader,
    /// VeraCrypt: the PIM, 0 for the default.
    #[serde(default)]
    pub pim: u32,
    /// VeraCrypt: only this key derivation.
    #[serde(default)]
    pub prf: Option<VeracryptPrf>,
    /// VeraCrypt: host paths of keyfiles or keyfile directories.
    #[serde(default)]
    pub keyfiles: Vec<String>,
    /// Optional newline-delimited wordlist file tested in parallel via rayon.
    #[serde(default)]
    pub wordlist: Option<String>,
    pub logical_volume_index: Option<usize>,
    pub encrypted_root_plist_path: Option<String>,
}

impl Drop for UnlockOptions {
    fn drop(&mut self) {
        for path in &mut self.keyfiles {
            path.zeroize();
        }
        if let Some(wl) = &mut self.wordlist {
            wl.zeroize();
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedKeyslotInfo {
    pub slot_id: String,
    pub kdf: String,
    pub iterations: Option<u64>,
    pub memory_kb: Option<u64>,
    pub parallelism: Option<u32>,
    pub af_stripes: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedVolumeInspect {
    pub detected_scheme: String,
    pub offset: u64,
    pub length: u64,
    pub version: Option<u16>,
    pub uuid: Option<String>,
    pub cipher: Option<String>,
    pub sector_size: Option<usize>,
    pub key_bits: Option<u32>,
    pub payload_offset: Option<u64>,
    pub keyslots: Vec<EncryptedKeyslotInfo>,
    pub veracrypt_primary_entropy: Option<f64>,
    pub veracrypt_hidden_header_offset: Option<u64>,
    pub veracrypt_hidden_entropy: Option<f64>,
    pub veracrypt_backup_header_offset: Option<u64>,
    pub filevault_volumes: Vec<String>,
    pub notes: Vec<String>,
}
#[derive(Serialize)]
pub struct UnlockResult {
    #[serde(skip)]
    pub(crate) read_cancellation: CancellationToken,
    pub image: ImageInfo,
    pub scheme: EncryptionScheme,
    pub cipher: String,
    pub encryption_sector_size: usize,
    pub source_offset: u64,
    pub source_length: u64,
    pub hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_volume_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_volume_index: Option<usize>,
    /// The BitLocker recovery password a VMK credential yielded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_password: Option<String>,
    /// What the VeraCrypt or TrueCrypt header that opened says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub veracrypt: Option<VeracryptDetails>,
}

enum EvidenceBacking {
    Disk(Weak<DiskImage>),
    Node {
        session: Weak<EvidenceSession>,
        node_id: String,
    },
}

#[derive(Clone, Copy)]
enum ReaderSpec<'a> {
    Disk(&'a Arc<DiskImage>),
    Session(&'a Arc<EvidenceSession>),
}

impl<'a> ReaderSpec<'a> {
    fn image(self) -> &'a Arc<DiskImage> {
        match self {
            Self::Disk(image) => image,
            Self::Session(session) => session.disk_image(),
        }
    }

    fn open(
        self,
        node_id: Option<&str>,
        partition_id: Option<u8>,
        offset: Option<u64>,
        length: Option<u64>,
        cancellation: Arc<Mutex<CancellationToken>>,
    ) -> anyhow::Result<EvidenceReader> {
        match self {
            Self::Disk(image) => {
                ensure!(
                    node_id.is_none(),
                    "nodeId requires an active EvidenceSession"
                );
                EvidenceReader::new(image, partition_id, offset, length, cancellation)
            }
            Self::Session(session) => EvidenceReader::new_for_session(
                session,
                node_id,
                partition_id,
                offset,
                length,
                cancellation,
            ),
        }
    }
}

pub(crate) struct EvidenceReader {
    backing: EvidenceBacking,
    offset: u64,
    length: u64,
    position: u64,
    cancellation: Arc<Mutex<CancellationToken>>,
}

impl EvidenceReader {
    pub(crate) fn bounds(&self) -> (u64, u64) {
        (self.offset, self.length)
    }

    pub(crate) fn new(
        source: &Arc<DiskImage>,
        partition_id: Option<u8>,
        offset: Option<u64>,
        length: Option<u64>,
        cancellation: Arc<Mutex<CancellationToken>>,
    ) -> anyhow::Result<Self> {
        let (offset, length) = if let Some(id) = partition_id {
            ensure!(
                offset.is_none() && length.is_none(),
                "partitionId cannot be combined with offset/length"
            );
            partition_range(source, id)?
        } else {
            let offset = offset.unwrap_or(0);
            (
                offset,
                length.unwrap_or_else(|| source.len().saturating_sub(offset)),
            )
        };
        ensure!(
            length >= 512
                && offset
                    .checked_add(length)
                    .is_some_and(|end| end <= source.len()),
            "encrypted region is outside the image"
        );
        Ok(Self {
            backing: EvidenceBacking::Disk(Arc::downgrade(source)),
            offset,
            length,
            position: 0,
            cancellation,
        })
    }

    pub(crate) fn new_for_session(
        session: &Arc<EvidenceSession>,
        node_id: Option<&str>,
        partition_id: Option<u8>,
        offset: Option<u64>,
        length: Option<u64>,
        cancellation: Arc<Mutex<CancellationToken>>,
    ) -> anyhow::Result<Self> {
        let Some(nid) = node_id else {
            return Self::new(
                session.disk_image(),
                partition_id,
                offset,
                length,
                cancellation,
            );
        };
        ensure!(
            partition_id.is_none(),
            "nodeId cannot be combined with partitionId"
        );
        let stat = fs_tree_read::file_stat(session, nid)
            .with_context(|| format!("encrypted container node not found: {nid}"))?;
        ensure!(
            !stat.mode.is_some_and(|m| (m & 0o170_000) == 0o040_000),
            "encrypted container node must be a file"
        );
        let off = offset.unwrap_or(0);
        let len = length.unwrap_or_else(|| stat.size.saturating_sub(off));
        ensure!(
            len >= 512 && off.checked_add(len).is_some_and(|end| end <= stat.size),
            "encrypted region is outside the node file"
        );
        Ok(Self {
            backing: EvidenceBacking::Node {
                session: Arc::downgrade(session),
                node_id: nid.to_owned(),
            },
            offset: off,
            length: len,
            position: 0,
            cancellation,
        })
    }

    fn check_cancelled(&self) -> std::io::Result<()> {
        if self
            .cancellation
            .lock()
            .map_err(|_| std::io::Error::other("encrypted cancellation lock poisoned"))?
            .is_cancelled()
        {
            return Err(std::io::Error::other(
                "encrypted volume operation cancelled",
            ));
        }
        Ok(())
    }

    /// Fill `buffer` from region offset `offset`; short reads and
    /// synthesized (bad-sector) bytes fail rather than become ciphertext.
    fn fill(&self, offset: u64, buffer: &mut [u8]) -> std::io::Result<()> {
        let absolute = self.offset + offset;
        match &self.backing {
            EvidenceBacking::Disk(source) => {
                let source = source
                    .upgrade()
                    .ok_or_else(|| std::io::Error::other("encrypted evidence closed"))?;
                let filled = source
                    .read_into(absolute, buffer)
                    .map_err(std::io::Error::other)?;
                if filled != buffer.len()
                    || source.has_bad_region_in_range(absolute, buffer.len() as u64)
                {
                    return Err(std::io::Error::other(
                        "encrypted evidence contains unreadable ciphertext",
                    ));
                }
            }
            EvidenceBacking::Node { session, node_id } => {
                let session = session
                    .upgrade()
                    .ok_or_else(|| std::io::Error::other("encrypted evidence session closed"))?;
                let bytes =
                    fs_tree_read::read_file_range(&session, node_id, absolute, buffer.len())
                        .map_err(std::io::Error::other)?;
                if bytes.len() != buffer.len() {
                    return Err(std::io::Error::other(
                        "encrypted container file contains unreadable or truncated range",
                    ));
                }
                buffer.copy_from_slice(&bytes);
            }
        }
        Ok(())
    }
}

/// The byte range of partition `id` in `source`.
pub(super) fn partition_range(source: &DiskImage, id: u8) -> anyhow::Result<(u64, u64)> {
    let partition = source
        .partitions
        .iter()
        .find(|partition| partition.id == id)
        .context(Failure::not_found("partition not found"))?;
    Ok((
        partition
            .start_lba
            .checked_mul(source.sector_size as u64)
            .context(Failure::corrupt("partition offset overflow"))?,
        partition
            .length_lba
            .checked_mul(source.sector_size as u64)
            .context(Failure::corrupt("partition size overflow"))?,
    ))
}

impl Read for EvidenceReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.check_cancelled()?;
        let count = buffer
            .len()
            .min(self.length.saturating_sub(self.position) as usize);
        if count == 0 {
            return Ok(0);
        }
        self.fill(self.position, &mut buffer[..count])?;
        self.position += count as u64;
        Ok(count)
    }
}

/// VeraCrypt reads ciphertext at explicit offsets, from several threads.
impl veracrypt::ReadAt for EvidenceReader {
    fn size(&self) -> u64 {
        self.length
    }

    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> std::io::Result<()> {
        self.check_cancelled()?;
        if offset
            .checked_add(buffer.len() as u64)
            .is_none_or(|end| end > self.length)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "read past the encrypted region",
            ));
        }
        self.fill(offset, buffer)
    }
}

impl Seek for EvidenceReader {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        if self
            .cancellation
            .lock()
            .map_err(|_| std::io::Error::other("encrypted cancellation lock poisoned"))?
            .is_cancelled()
        {
            return Err(std::io::Error::other(
                "encrypted volume operation cancelled",
            ));
        }
        let position = match from {
            SeekFrom::Start(position) => i128::from(position),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
            SeekFrom::End(delta) => i128::from(self.length) + i128::from(delta),
        };
        self.position = u64::try_from(position)
            .map_err(|_| std::io::Error::other("invalid encrypted evidence seek"))?;
        Ok(self.position)
    }
}

/// A plaintext reader that moves a cursor, so reads take turns.
pub(super) enum PlaintextReader {
    Bitlocker {
        volume: Box<crate::core::evidence::crypto::bde_image::BdeVolume>,
        _ciphertext: Arc<bitlocker::Ciphertext>,
    },
    Luks(Box<luks::DecryptedPayload<EvidenceReader>>),
    Filevault(Box<filevault::FileVaultVolume<EvidenceReader>>),
}

pub(super) enum Plaintext {
    Locked(Mutex<PlaintextReader>),
    /// VeraCrypt reads at explicit offsets and decrypts on the caller's
    /// thread, so concurrent reads do not wait for each other.
    Veracrypt(Box<veracrypt::Volume<EvidenceReader>>),
}

impl Plaintext {
    fn locked(reader: PlaintextReader) -> Self {
        Self::Locked(Mutex::new(reader))
    }
}

struct PlaintextSource {
    plaintext: Plaintext,
    length: u64,
}

impl SectorSource for PlaintextSource {
    fn size(&self) -> u64 {
        self.length
    }

    fn read(&self, offset: u64, length: usize) -> anyhow::Result<Vec<u8>> {
        let length = (length as u64).min(self.length.saturating_sub(offset)) as usize;
        let mut bytes = vec![0; length];
        let offset = offset.min(self.length);
        let reader = match &self.plaintext {
            Plaintext::Veracrypt(volume) => {
                volume
                    .read_at(offset, &mut bytes)
                    .map_err(veracrypt_volume::map_error)?;
                return Ok(bytes);
            }
            Plaintext::Locked(reader) => reader,
        };
        let mut reader = reader
            .lock()
            .map_err(|_| anyhow!("decrypted reader lock poisoned"))?;
        match &mut *reader {
            PlaintextReader::Bitlocker { volume, .. } => {
                let count = volume.read_at(offset, &mut bytes)?;
                ensure!(count == length, "short BitLocker plaintext read");
            }
            PlaintextReader::Luks(reader) => reader.read_at(offset, &mut bytes)?,
            PlaintextReader::Filevault(reader) => {
                let count = reader.read_at(offset, &mut bytes)?;
                ensure!(count == length, "short FileVault plaintext read");
            }
        }
        Ok(bytes)
    }
}

/// How an opened plaintext is laid out.
#[derive(Clone, Copy)]
pub(super) enum PlaintextLayout {
    /// A disk: its partition table, or a filesystem filling it, is detected.
    Disk,
    /// Exactly one filesystem volume, whatever its contents look like.
    Volume,
}

/// The FileVault logical volume a container unlock opened.
struct LogicalVolume {
    index: usize,
    identifier: Option<String>,
    name: Option<String>,
}

/// One scheme's opened volume, before it becomes an image.
pub(super) struct OpenedVolume {
    plaintext: Plaintext,
    length: u64,
    cipher: String,
    encryption_sector_size: usize,
    pub(super) layout: PlaintextLayout,
    logical_volume: Option<LogicalVolume>,
    recovery_password: Option<String>,
    pub(super) hidden: bool,
    pub(super) veracrypt: Option<VeracryptDetails>,
    /// The source bytes the plaintext stands for, when they are not the
    /// region the reader covers (a system partition read through its drive).
    pub(super) source_range: Option<(u64, u64)>,
}

impl OpenedVolume {
    pub(super) fn new(
        plaintext: Plaintext,
        length: u64,
        cipher: String,
        encryption_sector_size: usize,
    ) -> Self {
        Self {
            plaintext,
            length,
            cipher,
            encryption_sector_size,
            layout: PlaintextLayout::Disk,
            logical_volume: None,
            recovery_password: None,
            hidden: false,
            veracrypt: None,
            source_range: None,
        }
    }
}

pub fn unlock(
    source: &Arc<DiskImage>,
    options: UnlockOptions,
    cancellation: CancellationToken,
) -> anyhow::Result<(DiskImage, UnlockResult)> {
    unlock_with_progress(source, options, cancellation, &|_, _| {})
}

/// [`unlock`], reporting `(done, total)` key derivation jobs while a
/// VeraCrypt header search runs.
pub fn unlock_with_progress(
    source: &Arc<DiskImage>,
    options: UnlockOptions,
    cancellation: CancellationToken,
    progress: &(dyn Fn(u64, u64) + Sync),
) -> anyhow::Result<(DiskImage, UnlockResult)> {
    unlock_with_spec_and_progress(ReaderSpec::Disk(source), options, cancellation, progress)
}

pub fn unlock_session(
    session: &Arc<EvidenceSession>,
    options: UnlockOptions,
    cancellation: CancellationToken,
) -> anyhow::Result<(DiskImage, UnlockResult)> {
    unlock_session_with_progress(session, options, cancellation, &|_, _| {})
}

pub fn unlock_session_with_progress(
    session: &Arc<EvidenceSession>,
    options: UnlockOptions,
    cancellation: CancellationToken,
    progress: &(dyn Fn(u64, u64) + Sync),
) -> anyhow::Result<(DiskImage, UnlockResult)> {
    unlock_with_spec_and_progress(
        ReaderSpec::Session(session),
        options,
        cancellation,
        progress,
    )
}

fn unlock_with_spec_and_progress(
    spec: ReaderSpec<'_>,
    options: UnlockOptions,
    cancellation: CancellationToken,
    progress: &(dyn Fn(u64, u64) + Sync),
) -> anyhow::Result<(DiskImage, UnlockResult)> {
    ensure!(
        !cancellation.is_cancelled(),
        Failure::cancelled("encrypted volume operation cancelled")
    );
    ensure!(
        options.node_id.is_none() || options.target == UnlockTarget::NewImage,
        "文件级加密容器只能以新证据镜像方式解锁"
    );
    ensure!(
        options.node_id.is_none()
            || !(options.scheme == EncryptionScheme::Veracrypt
                && options.header == VeracryptHeader::System),
        Failure::invalid_input("a system encryption header requires a whole disk image")
    );
    check_options(&options)?;

    if let Some(wordlist_path) = options.wordlist.as_deref() {
        let matched = find_wordlist_password(spec, &options, wordlist_path, &cancellation)?;
        let exact_opts = UnlockOptions {
            target: options.target,
            scheme: options.scheme,
            credential: UnlockCredential::Password {
                password: matched.as_str().to_string(),
            },
            partition_id: options.partition_id,
            node_id: options.node_id.clone(),
            offset: options.offset,
            length: options.length,
            header: options.header,
            pim: options.pim,
            prf: options.prf,
            keyfiles: options.keyfiles.clone(),
            wordlist: None,
            logical_volume_index: options.logical_volume_index,
            encrypted_root_plist_path: options.encrypted_root_plist_path.clone(),
        };
        return unlock_with_spec_core(spec, &exact_opts, cancellation, progress);
    }

    unlock_with_spec_core(spec, &options, cancellation, progress)
}

pub fn unlock_and_return_matched_password(
    source: &Arc<DiskImage>,
    options: UnlockOptions,
    cancellation: CancellationToken,
) -> anyhow::Result<(DiskImage, UnlockResult, Option<String>)> {
    unlock_and_return_matched_password_with_spec(ReaderSpec::Disk(source), options, cancellation)
}

pub fn unlock_session_and_return_matched_password(
    session: &Arc<EvidenceSession>,
    options: UnlockOptions,
    cancellation: CancellationToken,
) -> anyhow::Result<(DiskImage, UnlockResult, Option<String>)> {
    unlock_and_return_matched_password_with_spec(
        ReaderSpec::Session(session),
        options,
        cancellation,
    )
}

fn unlock_and_return_matched_password_with_spec(
    spec: ReaderSpec<'_>,
    options: UnlockOptions,
    cancellation: CancellationToken,
) -> anyhow::Result<(DiskImage, UnlockResult, Option<String>)> {
    ensure!(
        options.node_id.is_none() || options.target == UnlockTarget::NewImage,
        "文件级加密容器只能以新证据镜像方式解锁"
    );
    ensure!(
        options.node_id.is_none()
            || !(options.scheme == EncryptionScheme::Veracrypt
                && options.header == VeracryptHeader::System),
        Failure::invalid_input("a system encryption header requires a whole disk image")
    );
    check_options(&options)?;
    if let Some(wordlist_path) = options.wordlist.as_deref() {
        let matched = find_wordlist_password(spec, &options, wordlist_path, &cancellation)?;
        let exact_opts = UnlockOptions {
            target: options.target,
            scheme: options.scheme,
            credential: UnlockCredential::Password {
                password: matched.as_str().to_string(),
            },
            partition_id: options.partition_id,
            node_id: options.node_id.clone(),
            offset: options.offset,
            length: options.length,
            header: options.header,
            pim: options.pim,
            prf: options.prf,
            keyfiles: options.keyfiles.clone(),
            wordlist: None,
            logical_volume_index: options.logical_volume_index,
            encrypted_root_plist_path: options.encrypted_root_plist_path.clone(),
        };
        let (img, res) = unlock_with_spec_core(spec, &exact_opts, cancellation, &|_, _| {})?;
        return Ok((img, res, Some(matched.to_string())));
    }
    let (img, res) = unlock_with_spec_core(spec, &options, cancellation, &|_, _| {})?;
    Ok((img, res, None))
}

fn load_wordlist(path: &str) -> anyhow::Result<Vec<Zeroizing<String>>> {
    let raw = std::fs::read(path).with_context(|| format!("failed to read wordlist '{path}'"))?;
    let text = String::from_utf8_lossy(&raw);
    let mut seen = HashSet::new();
    let mut words = Vec::new();
    for line in text.lines() {
        let cleaned = line.trim_end_matches('\r');
        if !cleaned.is_empty() && seen.insert(cleaned.to_string()) {
            words.push(Zeroizing::new(cleaned.to_string()));
        }
        let trimmed = cleaned.trim();
        if !trimmed.is_empty() && trimmed != cleaned && seen.insert(trimmed.to_string()) {
            words.push(Zeroizing::new(trimmed.to_string()));
        }
    }
    ensure!(!words.is_empty(), "wordlist '{path}' contains no entries");
    Ok(words)
}

fn find_wordlist_password(
    spec: ReaderSpec<'_>,
    options: &UnlockOptions,
    wordlist_path: &str,
    cancellation: &CancellationToken,
) -> anyhow::Result<Zeroizing<String>> {
    let words = load_wordlist(wordlist_path)?;
    let cancel_token = Arc::new(Mutex::new(cancellation.clone()));
    let mut probe_reader = spec.open(
        options.node_id.as_deref(),
        options.partition_id,
        options.offset,
        options.length,
        cancel_token,
    )?;
    let length = probe_reader.length;

    let found: Mutex<Option<Zeroizing<String>>> = Mutex::new(None);
    let stop = AtomicBool::new(false);
    let source = spec.image();

    match options.scheme {
        EncryptionScheme::Bitlocker => {
            bail!(Failure::invalid_input(
                "wordlist cracking is not supported for BitLocker"
            ));
        }
        EncryptionScheme::Veracrypt => {
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) || cancellation.is_cancelled() {
                    return;
                }
                let candidate_opts = UnlockOptions {
                    target: options.target,
                    scheme: EncryptionScheme::Veracrypt,
                    credential: UnlockCredential::Password {
                        password: pw.as_str().to_string(),
                    },
                    partition_id: options.partition_id,
                    node_id: options.node_id.clone(),
                    offset: options.offset,
                    length: options.length,
                    header: options.header,
                    pim: options.pim,
                    prf: options.prf,
                    keyfiles: options.keyfiles.clone(),
                    wordlist: None,
                    logical_volume_index: None,
                    encrypted_root_plist_path: None,
                };
                if let Ok(real) = spec.open(
                    options.node_id.as_deref(),
                    options.partition_id,
                    options.offset,
                    options.length,
                    Arc::new(Mutex::new(cancellation.clone())),
                ) {
                    if veracrypt_volume::open(
                        source,
                        real,
                        &candidate_opts,
                        cancellation,
                        &|_, _| {},
                    )
                    .is_ok()
                    {
                        stop.store(true, Ordering::SeqCst);
                        if let Ok(mut g) = found.lock() {
                            *g = Some(pw.clone());
                        }
                    }
                }
            });
        }
        EncryptionScheme::Luks => {
            crate::core::evidence::crypto::luks_metadata::validate(&mut probe_reader, length)?;
            probe_reader.rewind()?;
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) || cancellation.is_cancelled() {
                    return;
                }
                if let Ok(real) = spec.open(
                    options.node_id.as_deref(),
                    options.partition_id,
                    options.offset,
                    options.length,
                    Arc::new(Mutex::new(cancellation.clone())),
                ) {
                    if luks::LuksVolume::unlock_with_passphrase(real, pw.as_bytes()).is_ok() {
                        stop.store(true, Ordering::SeqCst);
                        if let Ok(mut g) = found.lock() {
                            *g = Some(pw.clone());
                        }
                    }
                }
            });
        }
        EncryptionScheme::Filevault => {
            let external = super::filevault::read_external_context(
                options.encrypted_root_plist_path.as_deref(),
            )?;
            let index = options.logical_volume_index.unwrap_or(0);
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) || cancellation.is_cancelled() {
                    return;
                }
                if let Ok(real) = spec.open(
                    options.node_id.as_deref(),
                    options.partition_id,
                    options.offset,
                    options.length,
                    Arc::new(Mutex::new(cancellation.clone())),
                ) {
                    if let Ok(container) =
                        filevault::storage::Container::open(real, external.as_deref())
                    {
                        if container
                            .unlock_with_password_cancellable(index, pw.as_str(), || {
                                cancellation.is_cancelled()
                            })
                            .is_ok()
                        {
                            stop.store(true, Ordering::SeqCst);
                            if let Ok(mut g) = found.lock() {
                                *g = Some(pw.clone());
                            }
                        }
                    }
                }
            });
        }
    }

    ensure!(!cancellation.is_cancelled(), "wordlist crack cancelled");
    found
        .into_inner()
        .map_err(|_| anyhow!("wordlist lock poisoned"))?
        .ok_or_else(|| anyhow!("未能从字典 '{wordlist_path}' 中匹配到有效密码"))
}

fn unlock_with_spec_core(
    spec: ReaderSpec<'_>,
    options: &UnlockOptions,
    cancellation: CancellationToken,
    progress: &(dyn Fn(u64, u64) + Sync),
) -> anyhow::Result<(DiskImage, UnlockResult)> {
    let source = spec.image();
    let read_cancellation = Arc::new(Mutex::new(cancellation.clone()));
    // A system encryption header sits in sector 62 of the drive, outside the
    // partition it protects: read the whole drive.
    let system =
        options.scheme == EncryptionScheme::Veracrypt && options.header == VeracryptHeader::System;
    let reader = if system {
        EvidenceReader::new(
            source,
            None,
            Some(0),
            Some(source.len()),
            Arc::clone(&read_cancellation),
        )?
    } else {
        spec.open(
            options.node_id.as_deref(),
            options.partition_id,
            options.offset,
            options.length,
            Arc::clone(&read_cancellation),
        )?
    };
    let (offset, source_length) = reader.bounds();
    let opened = match options.scheme {
        EncryptionScheme::Bitlocker => {
            let unlocked = bitlocker::unlock(
                source,
                offset,
                source_length,
                &options.credential,
                Arc::clone(&read_cancellation),
            )?;
            let length = unlocked.volume.size();
            let mut opened = OpenedVolume::new(
                Plaintext::locked(PlaintextReader::Bitlocker {
                    volume: Box::new(unlocked.volume),
                    _ciphertext: unlocked.ciphertext,
                }),
                length,
                unlocked.cipher,
                unlocked.sector_size,
            );
            opened.layout = PlaintextLayout::Volume;
            opened.recovery_password = unlocked.recovery_password;
            opened
        }
        EncryptionScheme::Luks => open_luks(reader, options)?,
        EncryptionScheme::Veracrypt => {
            veracrypt_volume::open(source, reader, options, &cancellation, progress)?
        }
        EncryptionScheme::Filevault => open_filevault(reader, options, &cancellation)?,
    };
    if cancellation.is_cancelled() {
        bail!(Failure::cancelled("encrypted volume operation cancelled"));
    }
    let OpenedVolume {
        plaintext,
        length,
        cipher,
        encryption_sector_size,
        layout,
        logical_volume,
        recovery_password,
        hidden,
        veracrypt,
        source_range,
    } = opened;
    let (offset, source_length) = source_range.unwrap_or((offset, source_length));
    let plaintext = Arc::new(PlaintextSource { plaintext, length });
    let (partitions, table_scheme, disk_id) = match layout {
        PlaintextLayout::Disk => {
            let detected = detect_partitions_with_meta(plaintext.as_ref(), PLAINTEXT_SECTOR)?;
            (detected.partitions, detected.scheme, detected.disk_id)
        }
        PlaintextLayout::Volume => (vec![single_volume(plaintext.as_ref())?], None, None),
    };
    if options.scheme == EncryptionScheme::Filevault {
        ensure!(
            partitions.iter().any(|partition| matches!(
                partition.kind,
                PartitionKind::HfsPlus | PartitionKind::HfsX
            )),
            "FileVault plaintext does not contain a recognized HFS+ volume"
        );
    }
    let mut image =
        DiskImage::from_assembled("decrypted".into(), plaintext, partitions, PLAINTEXT_SECTOR)?;
    image.path = if options.node_id.is_some() {
        format!("decrypted://node/{}/{}", source.id, image.id).into()
    } else {
        format!("decrypted://{}", image.id).into()
    };
    if let (Some(name), Some(partition)) = (
        logical_volume
            .as_ref()
            .and_then(|volume| volume.name.clone()),
        image.partitions.first_mut(),
    ) {
        partition.label = Some(name);
    }
    image.derived = Some(DerivedImage::Decrypted {
        parent: source.id.clone(),
    });
    image.table_scheme = table_scheme;
    image.disk_id = disk_id;
    ensure!(
        !cancellation.is_cancelled(),
        Failure::cancelled("encrypted volume operation cancelled")
    );
    // Completed views have their own lifetime; a late unlock cancel must not relock them.
    let lifetime = CancellationToken::new();
    *read_cancellation
        .lock()
        .map_err(|_| anyhow!("encrypted cancellation lock poisoned"))? = lifetime.clone();
    let (logical_volume_index, logical_volume_identifier) = match logical_volume {
        Some(volume) => (Some(volume.index), volume.identifier),
        None => (None, None),
    };
    let result = UnlockResult {
        read_cancellation: lifetime,
        image: image.info(),
        scheme: options.scheme,
        cipher,
        encryption_sector_size,
        source_offset: offset,
        source_length,
        hidden,
        volume_key: None,
        logical_volume_identifier,
        logical_volume_index,
        recovery_password,
        veracrypt,
    };
    Ok((image, result))
}

fn byte_entropy(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for &b in bytes {
        counts[usize::from(b)] += 1;
    }
    let len = bytes.len() as f64;
    let mut ent = 0.0;
    for c in counts {
        if c > 0 {
            let p = f64::from(c) / len;
            ent -= p * p.log2();
        }
    }
    ent
}

/// Inspect an encrypted container header/region without unlocking it.
pub fn inspect(
    source: &Arc<DiskImage>,
    partition_id: Option<u8>,
    offset: Option<u64>,
    length: Option<u64>,
) -> anyhow::Result<EncryptedVolumeInspect> {
    inspect_with_spec(ReaderSpec::Disk(source), None, partition_id, offset, length)
}

/// Inspect an encrypted container header/region inside an [`EvidenceSession`] (including an in-tree `node_id`).
pub fn inspect_session(
    session: &Arc<EvidenceSession>,
    node_id: Option<&str>,
    partition_id: Option<u8>,
    offset: Option<u64>,
    length: Option<u64>,
) -> anyhow::Result<EncryptedVolumeInspect> {
    inspect_with_spec(
        ReaderSpec::Session(session),
        node_id,
        partition_id,
        offset,
        length,
    )
}

fn inspect_with_spec(
    spec: ReaderSpec<'_>,
    node_id: Option<&str>,
    partition_id: Option<u8>,
    offset: Option<u64>,
    length: Option<u64>,
) -> anyhow::Result<EncryptedVolumeInspect> {
    let cancel = Arc::new(Mutex::new(CancellationToken::new()));
    let mut reader = spec.open(node_id, partition_id, offset, length, cancel)?;
    let region_off = reader.offset;
    let region_len = reader.length;

    let mut head = [0u8; 4096];
    let head_len = region_len.min(4096) as usize;
    reader.read_exact(&mut head[..head_len])?;

    // 1. LUKS1 / LUKS2
    if head_len >= 592 && &head[..6] == b"LUKS\xba\xbe" {
        let ver = u16::from_be_bytes([head[6], head[7]]);
        if ver == 1 {
            let hdr = luks::Luks1Header::parse(&head[..592])?;
            let keyslots = hdr
                .active_keyslots()
                .enumerate()
                .map(|(i, s)| EncryptedKeyslotInfo {
                    slot_id: i.to_string(),
                    kdf: format!("pbkdf2-{}", hdr.hash_spec),
                    iterations: Some(u64::from(s.iterations)),
                    memory_kb: None,
                    parallelism: None,
                    af_stripes: Some(s.stripes),
                })
                .collect();
            return Ok(EncryptedVolumeInspect {
                detected_scheme: "luks1".to_string(),
                offset: region_off,
                length: region_len,
                version: Some(1),
                uuid: Some(hdr.uuid.clone()),
                cipher: Some(format!("{}-{}", hdr.cipher_name, hdr.cipher_mode)),
                sector_size: Some(512),
                key_bits: Some(hdr.key_bytes * 8),
                payload_offset: Some(hdr.payload_byte_offset()),
                keyslots,
                veracrypt_primary_entropy: None,
                veracrypt_hidden_header_offset: None,
                veracrypt_hidden_entropy: None,
                veracrypt_backup_header_offset: None,
                filevault_volumes: Vec::new(),
                notes: vec![format!("MK digest iterations: {}", hdr.mk_digest_iter)],
            });
        } else if ver == 2 && region_len >= 8192 {
            let json_len = u64::from_be_bytes(head[8..16].try_into().unwrap_or([0; 8]))
                .min(region_len)
                .min(4 * 1024 * 1024) as usize;
            let mut full = vec![0u8; json_len];
            reader.seek(SeekFrom::Start(0))?;
            reader.read_exact(&mut full)?;
            let json_bytes = full[4096..].split(|&b| b == 0).next().unwrap_or(&[]);
            if let Ok(meta) = serde_json::from_slice::<serde_json::Value>(json_bytes) {
                let mut keyslots = Vec::new();
                if let Some(obj) = meta["keyslots"].as_object() {
                    for (slot_id, slot) in obj {
                        let kdf_obj = &slot["kdf"];
                        let kdf_type = kdf_obj["type"].as_str().unwrap_or("unknown").to_string();
                        let iterations = kdf_obj["iterations"]
                            .as_u64()
                            .or_else(|| kdf_obj["time"].as_u64());
                        let memory_kb = kdf_obj["memory"].as_u64();
                        let parallelism =
                            kdf_obj["cpus"].as_u64().and_then(|v| u32::try_from(v).ok());
                        let stripes = slot["af"]["stripes"]
                            .as_u64()
                            .and_then(|v| u32::try_from(v).ok());
                        keyslots.push(EncryptedKeyslotInfo {
                            slot_id: slot_id.clone(),
                            kdf: kdf_type,
                            iterations,
                            memory_kb,
                            parallelism,
                            af_stripes: stripes,
                        });
                    }
                }
                let first_seg = meta["segments"].as_object().and_then(|m| m.values().next());
                let cipher = first_seg
                    .and_then(|s| s["encryption"].as_str())
                    .map(ToOwned::to_owned);
                let sector_size = first_seg
                    .and_then(|s| s["sector_size"].as_u64())
                    .and_then(|v| usize::try_from(v).ok());
                let payload_offset = first_seg
                    .and_then(|s| s["offset"].as_str().and_then(|st| st.parse::<u64>().ok()));
                let uuid_str = std::str::from_utf8(&full[168..208])
                    .ok()
                    .map(|s| s.trim_end_matches('\0').to_string());
                return Ok(EncryptedVolumeInspect {
                    detected_scheme: "luks2".to_string(),
                    offset: region_off,
                    length: region_len,
                    version: Some(2),
                    uuid: uuid_str,
                    cipher,
                    sector_size,
                    key_bits: None,
                    payload_offset,
                    keyslots,
                    veracrypt_primary_entropy: None,
                    veracrypt_hidden_header_offset: None,
                    veracrypt_hidden_entropy: None,
                    veracrypt_backup_header_offset: None,
                    filevault_volumes: Vec::new(),
                    notes: Vec::new(),
                });
            }
        }
    }

    // 2. FileVault 2 CoreStorage
    reader.rewind()?;
    if let Ok(container) = filevault::storage::Container::open(reader, None) {
        let volumes: Vec<String> = container
            .volumes()
            .map(|v| {
                format!(
                    "{} ({} bytes)",
                    v.info
                        .lv_identifier
                        .as_deref()
                        .or(v.info.lv_name.as_deref())
                        .unwrap_or("unknown"),
                    v.info.lv_size
                )
            })
            .collect();
        if !volumes.is_empty() {
            return Ok(EncryptedVolumeInspect {
                detected_scheme: "filevault2".to_string(),
                offset: region_off,
                length: region_len,
                version: Some(2),
                uuid: None,
                cipher: Some("AES-XTS-128".to_string()),
                sector_size: Some(512),
                key_bits: Some(128),
                payload_offset: None,
                keyslots: Vec::new(),
                veracrypt_primary_entropy: None,
                veracrypt_hidden_header_offset: None,
                veracrypt_hidden_entropy: None,
                veracrypt_backup_header_offset: None,
                filevault_volumes: volumes,
                notes: vec!["CoreStorage logical volumes detected".to_string()],
            });
        }
    }

    // 3. VeraCrypt / TrueCrypt high-entropy header & hidden/backup offset inspection
    let primary_entropy = byte_entropy(&head[..head_len.min(512)]);
    let mut hidden_off = None;
    let mut hidden_entropy = None;
    let mut backup_off = None;
    let mut notes = vec![
        "VeraCrypt/TrueCrypt headers are fully encrypted without cleartext magic.".to_string(),
        "Default PRF PIM iterations: SHA-512=500000, SHA-256=500000, Whirlpool=500000, RIPEMD-160=655331; with PIM>0: 15000 + PIM*1000 (or PIM*2048 for SHA-256/512/Whirlpool boot).".to_string(),
    ];
    if region_len >= 65_536 + 512 {
        let mut r2 = spec.open(
            node_id,
            partition_id,
            offset,
            length,
            Arc::new(Mutex::new(CancellationToken::new())),
        )?;
        let mut hbuf = [0u8; 512];
        r2.seek(SeekFrom::Start(65_536))?;
        if r2.read_exact(&mut hbuf).is_ok() {
            let ent = byte_entropy(&hbuf);
            hidden_off = Some(region_off + 65_536);
            hidden_entropy = Some((ent * 1000.0).round() / 1000.0);
            if ent > 7.2 {
                notes.push(format!(
                    "High-entropy block at +64 KiB (H={ent:.3}): potential VeraCrypt hidden volume header"
                ));
            }
        }
    }
    if region_len >= 131_072 {
        backup_off = Some(region_off + region_len - 131_072);
    }

    Ok(EncryptedVolumeInspect {
        detected_scheme: if primary_entropy > 7.2 {
            "veracrypt-candidate".to_string()
        } else {
            "unknown".to_string()
        },
        offset: region_off,
        length: region_len,
        version: None,
        uuid: None,
        cipher: Some("AES / Serpent / Twofish / Cascades (XTS-512)".to_string()),
        sector_size: Some(512),
        key_bits: Some(512),
        payload_offset: (region_len > 131_072).then_some(region_off + 131_072),
        keyslots: Vec::new(),
        veracrypt_primary_entropy: Some((primary_entropy * 1000.0).round() / 1000.0),
        veracrypt_hidden_header_offset: hidden_off,
        veracrypt_hidden_entropy: hidden_entropy,
        veracrypt_backup_header_offset: backup_off,
        filevault_volumes: Vec::new(),
        notes,
    })
}

/// Options that cannot apply to the chosen scheme fail before any read.
fn check_options(options: &UnlockOptions) -> anyhow::Result<()> {
    let scheme = options.scheme;
    ensure!(
        options.credential.opens(scheme),
        Failure::invalid_input(format!(
            "a {} does not open {} volumes",
            options.credential.label(),
            scheme.label()
        ))
    );
    if let UnlockCredential::Password { password } = &options.credential {
        // A VeraCrypt volume may open with keyfiles alone; a wordlist crack supplies passwords from file.
        ensure!(
            !password.is_empty()
                || options.wordlist.is_some()
                || (scheme == EncryptionScheme::Veracrypt && !options.keyfiles.is_empty()),
            Failure::invalid_input("the password is empty")
        );
        ensure!(
            password.len() <= 4096,
            Failure::invalid_input("password exceeds 4096 bytes")
        );
    }
    ensure!(
        scheme == EncryptionScheme::Veracrypt
            || (options.header == VeracryptHeader::Auto
                && options.pim == 0
                && options.prf.is_none()
                && options.keyfiles.is_empty()),
        Failure::invalid_input(
            "headers, PIM, key derivations and keyfiles apply only to VeraCrypt"
        )
    );
    if options.header == VeracryptHeader::System {
        ensure!(
            options.offset.is_none() && options.length.is_none(),
            Failure::invalid_input("a system encryption header is read from the whole drive; choose a partition or the whole image")
        );
        ensure!(
            options.partition_id.is_some() || options.target == UnlockTarget::NewImage,
            Failure::invalid_input("a system-encrypted drive opens as new evidence")
        );
        ensure!(
            options.pim <= veracrypt::MAX_BOOT_PIM,
            Failure::invalid_input(format!(
                "system encryption PIM is at most {}",
                veracrypt::MAX_BOOT_PIM
            ))
        );
    }
    ensure!(
        options.pim <= veracrypt::MAX_PIM,
        Failure::invalid_input(format!("PIM is at most {}", veracrypt::MAX_PIM))
    );
    ensure!(
        options.keyfiles.len() <= veracrypt_volume::MAX_KEYFILES,
        Failure::resource_limit(format!(
            "more than {} VeraCrypt keyfiles",
            veracrypt_volume::MAX_KEYFILES
        ))
    );
    ensure!(
        scheme == EncryptionScheme::Filevault
            || (options.logical_volume_index.is_none()
                && options.encrypted_root_plist_path.is_none()),
        Failure::invalid_input(
            "logical volume / encryption context options apply only to FileVault"
        )
    );
    Ok(())
}

/// The error for a credential the scheme's opener cannot use; `check_options`
/// rejects these first.
pub(super) fn wrong_credential(
    credential: &UnlockCredential,
    scheme: EncryptionScheme,
) -> anyhow::Error {
    anyhow!(Failure::invalid_input(format!(
        "a {} does not open {} volumes",
        credential.label(),
        scheme.label()
    )))
}

fn open_luks(mut reader: EvidenceReader, options: &UnlockOptions) -> anyhow::Result<OpenedVolume> {
    let (_, length) = reader.bounds();
    crate::core::evidence::crypto::luks_metadata::validate(&mut reader, length)?;
    reader.rewind()?;
    let volume = match &options.credential {
        UnlockCredential::Password { password } => {
            luks::LuksVolume::unlock_with_passphrase(reader, password.as_bytes())?
        }
        UnlockCredential::MasterKey { key_hex, .. } => {
            ensure!(
                key_hex.len() <= 512
                    && key_hex.len().is_multiple_of(2)
                    && !key_hex.is_empty()
                    && key_hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
                Failure::invalid_input("invalid LUKS master key hex")
            );
            let key = zeroize::Zeroizing::new(
                hex::decode(key_hex)
                    .context(Failure::invalid_input("invalid LUKS master key hex"))?,
            );
            luks::LuksVolume::unlock_with_master_key(reader, &key)?
        }
        credential @ (UnlockCredential::RecoveryPassword { .. }
        | UnlockCredential::StartupKey { .. }
        | UnlockCredential::Vmk { .. }
        | UnlockCredential::ClearKey) => {
            return Err(wrong_credential(credential, EncryptionScheme::Luks))
        }
    };
    let size = volume.payload_size();
    let info = volume.info();
    ensure!(
        size > 0 && size % info.sector_size as u64 == 0,
        Failure::corrupt("invalid LUKS payload geometry")
    );
    let cipher = format!(
        "LUKS{} {}-{}",
        info.version, info.cipher_name, info.cipher_mode
    );
    let sector = info.sector_size;
    Ok(OpenedVolume::new(
        Plaintext::locked(PlaintextReader::Luks(Box::new(volume))),
        size,
        cipher,
        sector,
    ))
}

fn open_filevault(
    reader: EvidenceReader,
    options: &UnlockOptions,
    cancellation: &CancellationToken,
) -> anyhow::Result<OpenedVolume> {
    let external =
        super::filevault::read_external_context(options.encrypted_root_plist_path.as_deref())?;
    let container = filevault::storage::Container::open(reader, external.as_deref())?;
    ensure!(
        container.volumes().count() == 1 || options.target == UnlockTarget::NewImage,
        "multiple FileVault logical volumes require independent newImage sessions"
    );
    let index = match options.logical_volume_index {
        Some(index) => index,
        None if container.volumes().count() == 1 => 0,
        None => bail!("FileVault contains multiple logical volumes; choose logicalVolumeIndex"),
    };
    let mut volume = match &options.credential {
        UnlockCredential::Password { password } => {
            container
                .unlock_with_password_cancellable(index, password, || cancellation.is_cancelled())?
        }
        UnlockCredential::MasterKey { key_hex, .. } => {
            ensure!(
                matches!(key_hex.len(), 32 | 64)
                    && key_hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
                Failure::invalid_input(
                    "FileVault recovered key must contain 16 or 32 bytes in hex"
                )
            );
            let key = zeroize::Zeroizing::new(hex::decode(key_hex)?);
            container.unlock_with_master_key(index, &key)?
        }
        credential @ (UnlockCredential::RecoveryPassword { .. }
        | UnlockCredential::StartupKey { .. }
        | UnlockCredential::Vmk { .. }
        | UnlockCredential::ClearKey) => {
            return Err(wrong_credential(credential, EncryptionScheme::Filevault))
        }
    };
    hfsplus::VolumeHeader::parse(&mut volume)
        .context("FileVault plaintext HFS+ header validation")?;
    let logical_volume = LogicalVolume {
        index,
        identifier: volume.info().lv_identifier.clone(),
        name: volume.info().lv_name.clone(),
    };
    let length = volume.size();
    let mut opened = OpenedVolume::new(
        Plaintext::locked(PlaintextReader::Filevault(Box::new(volume))),
        length,
        "FileVault 2 AES-XTS-128".to_string(),
        512,
    );
    opened.logical_volume = Some(logical_volume);
    Ok(opened)
}

/// The one partition of a plaintext that is a single volume: all of it, typed
/// the way a partition of that filesystem is (`Unknown` when none matches).
fn single_volume(plaintext: &dyn SectorSource) -> anyhow::Result<Partition> {
    let kind = match probe_volume(plaintext, PLAINTEXT_SECTOR)? {
        Some(hit) if hit.relative_offset == 0 => hit.kind,
        Some(_) | None => detect_filesystem(&plaintext.read(0, 4096)?, PartitionKind::Unknown),
    };
    Ok(Partition {
        id: 0,
        kind,
        start_lba: 0,
        length_lba: plaintext.size() / PLAINTEXT_SECTOR as u64,
        label: None,
        unique_guid: None,
        amiga_geometry: None,
        fs_label: None,
        fs_uuid: None,
    })
}
