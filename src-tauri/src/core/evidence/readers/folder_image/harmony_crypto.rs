//! Cryptographic primitives for HarmonyOS NEXT (`encryptionType == 2`) backups.
//!
//! Pure Rust implementation over `aes` and `sha2`:
//! - PBKDF2-HMAC-SHA256
//! - Arbitrary-nonce-length AES-256-GCM (NIST SP 800-38D GHASH J0 calculation)
//! - O(1) seekable AES-256-GCM-CTR range decryption at arbitrary byte offsets

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use aes::cipher::{BlockCipherEncrypt, KeyInit};
use aes::Aes256;
use anyhow::{bail, ensure, Context, Result};
use base64::Engine;
use sha2::{Digest, Sha256};

const GCM_REDUCTION_POLY: u128 = 0xe100_0000_0000_0000_0000_0000_0000_0000;

/// Computes HMAC-SHA256(key, data).
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k_pad = [0u8; 64];
    if key.len() > 64 {
        let digest = Sha256::digest(key);
        k_pad[..32].copy_from_slice(&digest);
    } else {
        k_pad[..key.len()].copy_from_slice(key);
    }

    let mut inner = Sha256::new();
    let mut ipad = [0x36u8; 64];
    for (dst, &k) in ipad.iter_mut().zip(k_pad.iter()) {
        *dst ^= k;
    }
    inner.update(ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    let mut opad = [0x5cu8; 64];
    for (dst, &k) in opad.iter_mut().zip(k_pad.iter()) {
        *dst ^= k;
    }
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

/// Computes PBKDF2-HMAC-SHA256(password, salt, iterations) into `out`.
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    let rounds = iterations.max(1);
    for (i, chunk) in out.chunks_mut(32).enumerate() {
        let block_idx = (i as u32) + 1;
        let mut salt_blk = Vec::with_capacity(salt.len() + 4);
        salt_blk.extend_from_slice(salt);
        salt_blk.extend_from_slice(&block_idx.to_be_bytes());
        let mut u = hmac_sha256(password, &salt_blk);
        let mut t = u;
        for _ in 1..rounds {
            u = hmac_sha256(password, &u);
            for (tb, ub) in t.iter_mut().zip(u.iter()) {
                *tb ^= *ub;
            }
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
}

/// GF(2^128) multiplication per NIST SP 800-38D Algorithm 1 (bit-reflected).
#[inline]
fn gf_mul(x: u128, y: u128) -> u128 {
    let mut z = 0u128;
    let mut v = x;
    for i in 0..128 {
        if ((y >> (127 - i)) & 1) == 1 {
            z ^= v;
        }
        let lsb = v & 1;
        v >>= 1;
        if lsb == 1 {
            v ^= GCM_REDUCTION_POLY;
        }
    }
    z
}

#[inline]
fn ghash_update_padded(mut state: u128, h: u128, data: &[u8]) -> u128 {
    for chunk in data.chunks(16) {
        let mut blk = [0u8; 16];
        blk[..chunk.len()].copy_from_slice(chunk);
        state = gf_mul(state ^ u128::from_be_bytes(blk), h);
    }
    state
}

#[inline]
fn ghash_block(state: u128, h: u128, block: [u8; 16]) -> u128 {
    gf_mul(state ^ u128::from_be_bytes(block), h)
}

#[inline]
pub fn inc32(counter: &[u8; 16], delta: u32) -> [u8; 16] {
    let mut out = *counter;
    let low = u32::from_be_bytes([out[12], out[13], out[14], out[15]]);
    out[12..16].copy_from_slice(&low.wrapping_add(delta).to_be_bytes());
    out
}

/// Computes `J0` for AES-256-GCM with an arbitrary nonce length.
pub fn compute_j0(key: &[u8; 32], iv: &[u8]) -> Result<[u8; 16]> {
    ensure!(!iv.is_empty(), "AES-GCM nonce cannot be empty");
    if iv.len() == 12 {
        let mut j0 = [0u8; 16];
        j0[..12].copy_from_slice(iv);
        j0[12..16].copy_from_slice(&1u32.to_be_bytes());
        return Ok(j0);
    }
    let cipher = Aes256::new_from_slice(key).context("invalid AES-256 key")?;
    let mut h_blk = aes::Block::from([0u8; 16]);
    cipher.encrypt_block(&mut h_blk);
    let h = u128::from_be_bytes(h_blk.into());

    let y = ghash_update_padded(0, h, iv);
    let mut len_blk = [0u8; 16];
    let iv_bits = (iv.len() as u64).saturating_mul(8);
    len_blk[8..16].copy_from_slice(&iv_bits.to_be_bytes());
    let j0_u128 = ghash_block(y, h, len_blk);
    Ok(j0_u128.to_be_bytes())
}

fn constant_time_eq(a: &[u8; 16], b: &[u8]) -> bool {
    if b.len() != 16 {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn apply_ctr_slice(cipher: &Aes256, j0: &[u8; 16], plain_offset: u64, buf: &mut [u8]) {
    let mut offset_in_buf = 0usize;
    let mut cur_plain = plain_offset;
    while offset_in_buf < buf.len() {
        let blk_idx = u32::try_from(cur_plain / 16).unwrap_or((cur_plain / 16) as u32);
        let in_blk = (cur_plain % 16) as usize;
        let ctr = inc32(j0, 1u32.wrapping_add(blk_idx));
        let mut ks = aes::Block::from(ctr);
        cipher.encrypt_block(&mut ks);
        let take = (16 - in_blk).min(buf.len() - offset_in_buf);
        for i in 0..take {
            buf[offset_in_buf + i] ^= ks[in_blk + i];
        }
        offset_in_buf += take;
        cur_plain += take as u64;
    }
}

/// Decrypts an in-memory AES-256-GCM `ciphertext || 16B tag` buffer and verifies the authentication tag.
pub fn aes256_gcm_decrypt_and_verify(
    key: &[u8; 32],
    iv: &[u8],
    cipher_and_tag: &[u8],
) -> Result<Vec<u8>> {
    ensure!(
        cipher_and_tag.len() >= 16,
        "AES-GCM ciphertext shorter than 16-byte tag"
    );
    let (ct, tag) = cipher_and_tag.split_at(cipher_and_tag.len() - 16);
    let cipher = Aes256::new_from_slice(key).context("invalid AES-256 key")?;
    let mut h_blk = aes::Block::from([0u8; 16]);
    cipher.encrypt_block(&mut h_blk);
    let h = u128::from_be_bytes(h_blk.into());
    let j0 = compute_j0(key, iv)?;

    let mut y = ghash_update_padded(0, h, ct);
    let mut len_blk = [0u8; 16];
    len_blk[8..16].copy_from_slice(&((ct.len() as u64) * 8).to_be_bytes());
    y = ghash_block(y, h, len_blk);

    let mut e_j0 = aes::Block::from(j0);
    cipher.encrypt_block(&mut e_j0);
    let expected_tag = (y ^ u128::from_be_bytes(e_j0.into())).to_be_bytes();
    if !constant_time_eq(&expected_tag, tag) {
        bail!("AES-256-GCM authentication tag mismatch");
    }

    let mut out = ct.to_vec();
    apply_ctr_slice(&cipher, &j0, 0, &mut out);
    Ok(out)
}

/// O(1) seekable AES-256-GCM-CTR decryption for a single contiguous plaintext byte range.
pub fn aes256_gcm_ctr_read_at(
    key: &[u8; 32],
    j0: &[u8; 16],
    path: &Path,
    base_plain_offset: u64,
    plain_len: u64,
    offset: u64,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    if offset >= plain_len || max_bytes == 0 {
        return Ok(Vec::new());
    }
    let want = usize::try_from((plain_len - offset).min(max_bytes as u64)).unwrap_or(max_bytes);
    let abs_plain_offset = base_plain_offset
        .checked_add(offset)
        .context("Harmony encrypted slice offset overflow")?;

    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    file.seek(SeekFrom::Start(abs_plain_offset))?;
    let mut buf = vec![0u8; want];
    let mut filled = 0usize;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);

    let cipher = Aes256::new_from_slice(key).context("invalid AES-256 key")?;
    apply_ctr_slice(&cipher, j0, abs_plain_offset, &mut buf);
    Ok(buf)
}

/// Decodes Base64 (standard/no-pad) OR comma-separated decimal bytes (`"66,1,190,..."`).
pub fn decode_harmony_bytes(raw: &str) -> Result<Vec<u8>> {
    let trimmed = raw.trim();
    if trimmed.contains(',') {
        let mut out = Vec::new();
        for part in trimmed.split(',') {
            let val: u8 = part
                .trim()
                .parse()
                .with_context(|| format!("invalid decimal byte '{part}'"))?;
            out.push(val);
        }
        return Ok(out);
    }
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| {
            base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed.trim_end_matches('='))
        })
        .context("invalid base64 in HarmonyOS backup metadata")
}

/// Layer 1 verification & unwrapping of `BackupFileTypeInfo.backupKey` (24B ASCII key).
pub fn unwrap_backup_key(
    password: &[u8],
    backup_salt_b64: &str,
    backup_key_b64: &str,
) -> Result<Vec<u8>> {
    let salt = decode_harmony_bytes(backup_salt_b64).context("decode backupSalt")?;
    let bk = decode_harmony_bytes(backup_key_b64).context("decode backupKey")?;
    ensure!(
        bk.len() >= 28 + 16,
        "backupKey blob too short ({} bytes)",
        bk.len()
    );
    let iv = &bk[12..28];
    let ct_and_tag = &bk[28..];
    let mut kek = [0u8; 32];
    pbkdf2_hmac_sha256(password, &salt, 10_000, &mut kek);
    aes256_gcm_decrypt_and_verify(&kek, iv, ct_and_tag)
        .context("HarmonyOS backup password verification failed")
}

/// Derives `(aes256_key, iv)` for one `BackupFileModuleInfo` entry.
pub fn derive_module_key_and_iv(
    bkey_raw: &[u8],
    encryption_salt: &str,
    encryption_iv: &str,
) -> Result<([u8; 32], Vec<u8>)> {
    let iv = decode_harmony_bytes(encryption_iv).context("decode module encryptionIv")?;
    ensure!(!iv.is_empty(), "empty module encryptionIv");
    if encryption_salt.contains(',') {
        let sym = decode_harmony_bytes(encryption_salt).context("decode module symkey")?;
        ensure!(
            sym.len() == 32,
            "expected 32-byte module symkey, got {}",
            sym.len()
        );
        let mut key = [0u8; 32];
        key.copy_from_slice(&sym);
        return Ok((key, iv));
    }
    let salt = decode_harmony_bytes(encryption_salt).context("decode module encryptionSalt")?;
    let mut key = [0u8; 32];
    pbkdf2_hmac_sha256(bkey_raw, &salt, 10_000, &mut key);
    Ok((key, iv))
}

#[cfg(test)]
pub(crate) fn aes256_gcm_encrypt_for_test(key: &[u8; 32], iv: &[u8], plain: &[u8]) -> Vec<u8> {
    let cipher = Aes256::new_from_slice(key).unwrap();
    let mut h_blk = aes::Block::from([0u8; 16]);
    cipher.encrypt_block(&mut h_blk);
    let h = u128::from_be_bytes(h_blk.into());
    let j0 = compute_j0(key, iv).unwrap();

    let mut out = plain.to_vec();
    apply_ctr_slice(&cipher, &j0, 0, &mut out);

    let mut y = ghash_update_padded(0, h, &out);
    let mut len_blk = [0u8; 16];
    len_blk[8..16].copy_from_slice(&((out.len() as u64) * 8).to_be_bytes());
    y = ghash_block(y, h, len_blk);

    let mut e_j0 = aes::Block::from(j0);
    cipher.encrypt_block(&mut e_j0);
    let tag = (y ^ u128::from_be_bytes(e_j0.into())).to_be_bytes();
    out.extend_from_slice(&tag);
    out
}
