//! Built-in multi-core forensic password & hash cracker (`rayon` parallel engine).
//!
//! Supports:
//! - Windows NTLM hashes (pure-Rust MD4 over UTF-16LE)
//! - Raw MD5 / SHA-1 / SHA-256 / SHA-512 hex digests
//! - Fernet v1 tokens (`gAAAAA...`, HMAC-SHA256 + AES-128-CBC with direct key / SHA-256 / PBKDF2)
//! - MetaMask Vault JSON (`{"data":...,"iv":...,"salt":...}`, PBKDF2-HMAC-SHA256 + AES-256-GCM)
//! - ZIP archives (WinZip AES-128/256 2-byte verifier & PKWARE ZipCrypto 12-byte header)
//! - Rule-based forensic candidate expansion + 4/6-digit PIN sweep + external wordlist

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use aes::{Aes128, Aes256};
use anyhow::{anyhow, bail, ensure, Context};
use base64::Engine;
use fast_md5::Md5;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k_block = [0u8; 64];
    if key.len() > 64 {
        let h = Sha256::digest(key);
        k_block[..32].copy_from_slice(&h);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k_block[i];
        opad[i] ^= k_block[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(data);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] {
    let mut k_block = [0u8; 64];
    if key.len() > 64 {
        let h = Sha1::digest(key);
        k_block[..20].copy_from_slice(&h);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k_block[i];
        opad[i] ^= k_block[i];
    }
    let mut inner = Sha1::new();
    inner.update(ipad);
    inner.update(data);
    let inner_hash = inner.finalize();
    let mut outer = Sha1::new();
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    for (i, chunk) in out.chunks_mut(32).enumerate() {
        let block_idx = (i as u32) + 1;
        let mut salt_blk = Vec::with_capacity(salt.len() + 4);
        salt_blk.extend_from_slice(salt);
        salt_blk.extend_from_slice(&block_idx.to_be_bytes());
        let mut u = hmac_sha256(password, &salt_blk);
        let mut t = u;
        for _ in 1..iterations.max(1) {
            u = hmac_sha256(password, &u);
            for (tb, ub) in t.iter_mut().zip(u.iter()) {
                *tb ^= *ub;
            }
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
}

fn pbkdf2_hmac_sha1(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    for (i, chunk) in out.chunks_mut(20).enumerate() {
        let block_idx = (i as u32) + 1;
        let mut salt_blk = Vec::with_capacity(salt.len() + 4);
        salt_blk.extend_from_slice(salt);
        salt_blk.extend_from_slice(&block_idx.to_be_bytes());
        let mut u = hmac_sha1(password, &salt_blk);
        let mut t = u;
        for _ in 1..iterations.max(1) {
            u = hmac_sha1(password, &u);
            for (tb, ub) in t.iter_mut().zip(u.iter()) {
                *tb ^= *ub;
            }
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
}

fn aes128_cbc_decrypt_pkcs7(key: &[u8; 16], iv: &[u8; 16], ciphertext: &[u8]) -> Option<Vec<u8>> {
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(16) {
        return None;
    }
    let cipher = Aes128::new_from_slice(key).ok()?;
    let mut out = Vec::with_capacity(ciphertext.len());
    let mut prev = *iv;
    let (chunks, _) = ciphertext.as_chunks::<16>();
    for chunk in chunks {
        let mut blk = aes::Block::from(*chunk);
        cipher.decrypt_block(&mut blk);
        for (b, p) in blk.iter().zip(prev.iter()) {
            out.push(*b ^ *p);
        }
        prev = *chunk;
    }
    let pad = *out.last()? as usize;
    if !(1..=16).contains(&pad) || pad > out.len() {
        return None;
    }
    if !out[out.len() - pad..].iter().all(|&b| b as usize == pad) {
        return None;
    }
    out.truncate(out.len() - pad);
    Some(out)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrackRequest {
    /// `"ntlm"` | `"md5"` | `"sha1"` | `"sha256"` | `"sha512"` | `"fernet"` | `"metamask"` | `"zip"`
    pub target_type: String,
    /// Target hash hex, Fernet token, MetaMask vault JSON, or hex/base64 ZIP bytes / host path.
    pub target: String,
    pub wordlist_path: Option<String>,
    #[serde(default)]
    pub candidates: Vec<String>,
    pub enable_rules: Option<bool>,
    pub pin_length: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrackResult {
    pub target_type: String,
    pub found: bool,
    pub password: Option<String>,
    pub decrypted_preview: Option<String>,
    pub tested_candidates: usize,
    pub elapsed_ms: u64,
}

/// Pure-Rust MD4 digest (RFC 1320) for NTLM hash computation.
pub fn md4_digest(input: &[u8]) -> [u8; 16] {
    #[inline]
    fn f(x: u32, y: u32, z: u32) -> u32 {
        (x & y) | (!x & z)
    }
    #[inline]
    fn g(x: u32, y: u32, z: u32) -> u32 {
        (x & y) | (x & z) | (y & z)
    }
    #[inline]
    fn h(x: u32, y: u32, z: u32) -> u32 {
        x ^ y ^ z
    }

    let mut msg = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    let mut a0: u32 = 0x6745_2301;
    let mut b0: u32 = 0xefcd_ab89;
    let mut c0: u32 = 0x98ba_dcfe;
    let mut d0: u32 = 0x1032_5476;

    for chunk in msg.chunks_exact(64) {
        let mut x = [0u32; 16];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            x[i] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);

        for &k in &[0usize, 4, 8, 12] {
            a = a.wrapping_add(f(b, c, d)).wrapping_add(x[k]).rotate_left(3);
            d = d
                .wrapping_add(f(a, b, c))
                .wrapping_add(x[k + 1])
                .rotate_left(7);
            c = c
                .wrapping_add(f(d, a, b))
                .wrapping_add(x[k + 2])
                .rotate_left(11);
            b = b
                .wrapping_add(f(c, d, a))
                .wrapping_add(x[k + 3])
                .rotate_left(19);
        }
        for &k in &[0usize, 1, 2, 3] {
            a = a
                .wrapping_add(g(b, c, d))
                .wrapping_add(x[k])
                .wrapping_add(0x5a82_7999)
                .rotate_left(3);
            d = d
                .wrapping_add(g(a, b, c))
                .wrapping_add(x[k + 4])
                .wrapping_add(0x5a82_7999)
                .rotate_left(5);
            c = c
                .wrapping_add(g(d, a, b))
                .wrapping_add(x[k + 8])
                .wrapping_add(0x5a82_7999)
                .rotate_left(9);
            b = b
                .wrapping_add(g(c, d, a))
                .wrapping_add(x[k + 12])
                .wrapping_add(0x5a82_7999)
                .rotate_left(13);
        }
        for &k in &[0usize, 2, 1, 3] {
            a = a
                .wrapping_add(h(b, c, d))
                .wrapping_add(x[k])
                .wrapping_add(0x6ed9_eba1)
                .rotate_left(3);
            d = d
                .wrapping_add(h(a, b, c))
                .wrapping_add(x[k + 8])
                .wrapping_add(0x6ed9_eba1)
                .rotate_left(9);
            c = c
                .wrapping_add(h(d, a, b))
                .wrapping_add(x[k + 4])
                .wrapping_add(0x6ed9_eba1)
                .rotate_left(11);
            b = b
                .wrapping_add(h(c, d, a))
                .wrapping_add(x[k + 12])
                .wrapping_add(0x6ed9_eba1)
                .rotate_left(15);
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

/// Compute Windows NTLM hash (`MD4(UTF-16LE(password))`).
pub fn ntlm_hash(password: &str) -> [u8; 16] {
    let mut utf16 = Vec::with_capacity(password.len() * 2);
    for u in password.encode_utf16() {
        utf16.extend_from_slice(&u.to_le_bytes());
    }
    md4_digest(&utf16)
}

pub fn build_candidate_list(req: &CrackRequest) -> anyhow::Result<Vec<String>> {
    let mut base_words = Vec::new();
    let mut seen = HashSet::new();

    for c in &req.candidates {
        let t = c.trim();
        if !t.is_empty() && seen.insert(t.to_string()) {
            base_words.push(t.to_string());
        }
    }

    if let Some(path) = req
        .wordlist_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let bytes = std::fs::read(path).with_context(|| format!("读取字典文件失败: {path}"))?;
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            let w = line.trim_end_matches('\r');
            if !w.is_empty() && seen.insert(w.to_string()) {
                base_words.push(w.to_string());
            }
        }
    }

    // Default forensic common passwords
    for common in [
        "123456",
        "password",
        "12345678",
        "admin",
        "root",
        "123456789",
        "1234",
        "P@ssw0rd",
        "Passw0rd",
        "Admin@123",
        "admin123",
        "root123",
        "111111",
        "000000",
        "666666",
        "888888",
        "qwerty",
        "abc123",
        "test",
        "guest",
        "flag",
        "secret",
    ] {
        if seen.insert(common.to_string()) {
            base_words.push(common.to_string());
        }
    }

    let mut final_words = Vec::with_capacity(base_words.len() * 4);
    let mut final_seen = HashSet::new();
    let apply_rules = req.enable_rules.unwrap_or(true);

    for w in base_words {
        if final_seen.insert(w.clone()) {
            final_words.push(w.clone());
        }
        if apply_rules && w.len() <= 32 {
            let mut chars = w.chars();
            let cap_opt = chars
                .next()
                .map(|first| format!("{}{}", first.to_uppercase(), chars.as_str()));
            if let Some(ref cap) = cap_opt {
                if final_seen.insert(cap.clone()) {
                    final_words.push(cap.clone());
                }
            }
            for suffix in ["123", "!", "@123", "2024", "2025", "2026", "888"] {
                let mutated = format!("{w}{suffix}");
                if final_seen.insert(mutated.clone()) {
                    final_words.push(mutated);
                }
                if let Some(ref cap) = cap_opt {
                    let cap_mut = format!("{cap}{suffix}");
                    if final_seen.insert(cap_mut.clone()) {
                        final_words.push(cap_mut);
                    }
                }
            }
        }
    }

    if let Some(pin_len) = req.pin_length {
        if pin_len == 4 {
            for n in 0..10_000u32 {
                let s = format!("{n:04}");
                if final_seen.insert(s.clone()) {
                    final_words.push(s);
                }
            }
        } else if pin_len == 6 {
            for n in 0..1_000_000u32 {
                let s = format!("{n:06}");
                if final_seen.insert(s.clone()) {
                    final_words.push(s);
                }
            }
        }
    }

    Ok(final_words)
}

pub fn run_crack(req: &CrackRequest) -> anyhow::Result<CrackResult> {
    let started = Instant::now();
    let words = build_candidate_list(req)?;
    let total = words.len();
    let kind = req.target_type.trim().to_ascii_lowercase();
    let target = req.target.trim();
    ensure!(!target.is_empty(), "破解目标不能为空");

    let stop = AtomicBool::new(false);
    let hit: Mutex<Option<(String, Option<String>)>> = Mutex::new(None);

    match kind.as_str() {
        "ntlm" => {
            let clean = target.rsplit(':').next().unwrap_or(target).trim();
            let expected: [u8; 16] = hex::decode(clean)
                .context("NTLM 哈希必须是 32 位十六进制字符串")?
                .try_into()
                .map_err(|_| anyhow!("NTLM 哈希必须是 16 字节（32 个十六进制字符）"))?;
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if ntlm_hash(pw) == expected {
                    stop.store(true, Ordering::SeqCst);
                    if let Ok(mut g) = hit.lock() {
                        *g = Some((pw.clone(), None));
                    }
                }
            });
        }
        "md5" => {
            let expected: [u8; 16] = hex::decode(target)
                .context("MD5 哈希必须是 32 位十六进制")?
                .try_into()
                .map_err(|_| anyhow!("MD5 长度必须是 16 字节"))?;
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let mut h = Md5::new();
                h.update(pw.as_bytes());
                let d = h.finalize();
                if d == expected {
                    stop.store(true, Ordering::SeqCst);
                    if let Ok(mut g) = hit.lock() {
                        *g = Some((pw.clone(), None));
                    }
                }
            });
        }
        "sha1" => {
            let expected: [u8; 20] = hex::decode(target)
                .context("SHA1 哈希必须是 40 位十六进制")?
                .try_into()
                .map_err(|_| anyhow!("SHA1 长度必须是 20 字节"))?;
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let d: [u8; 20] = Sha1::digest(pw.as_bytes()).into();
                if d == expected {
                    stop.store(true, Ordering::SeqCst);
                    if let Ok(mut g) = hit.lock() {
                        *g = Some((pw.clone(), None));
                    }
                }
            });
        }
        "sha256" => {
            let expected: [u8; 32] = hex::decode(target)
                .context("SHA256 哈希必须是 64 位十六进制")?
                .try_into()
                .map_err(|_| anyhow!("SHA256 长度必须是 32 字节"))?;
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let d: [u8; 32] = Sha256::digest(pw.as_bytes()).into();
                if d == expected {
                    stop.store(true, Ordering::SeqCst);
                    if let Ok(mut g) = hit.lock() {
                        *g = Some((pw.clone(), None));
                    }
                }
            });
        }
        "sha512" => {
            let expected: [u8; 64] = hex::decode(target)
                .context("SHA512 哈希必须是 128 位十六进制")?
                .try_into()
                .map_err(|_| anyhow!("SHA512 长度必须是 64 字节"))?;
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let d: [u8; 64] = Sha512::digest(pw.as_bytes()).into();
                if d == expected {
                    stop.store(true, Ordering::SeqCst);
                    if let Ok(mut g) = hit.lock() {
                        *g = Some((pw.clone(), None));
                    }
                }
            });
        }
        "fernet" => {
            let token_bytes = base64::engine::general_purpose::URL_SAFE
                .decode(target)
                .or_else(|_| {
                    base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(target.trim_end_matches('='))
                })
                .context("无效的 Fernet Base64URL 令牌")?;
            ensure!(
                token_bytes.len() >= 57
                    && token_bytes[0] == 0x80
                    && (token_bytes.len() - 57).is_multiple_of(16),
                "Fernet 令牌结构不合法"
            );
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(plain) = try_decrypt_fernet(&token_bytes, pw) {
                    stop.store(true, Ordering::SeqCst);
                    if let Ok(mut g) = hit.lock() {
                        *g = Some((pw.clone(), Some(plain)));
                    }
                }
            });
        }
        "metamask" => {
            let vault = parse_metamask_vault(target)?;
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(plain) = try_decrypt_metamask(&vault, pw) {
                    stop.store(true, Ordering::SeqCst);
                    if let Ok(mut g) = hit.lock() {
                        *g = Some((pw.clone(), Some(plain)));
                    }
                }
            });
        }
        "zip" => {
            let zip_bytes = if std::path::Path::new(target).exists() {
                std::fs::read(target).context("读取 ZIP 文件失败")?
            } else {
                hex::decode(target.strip_prefix("0x").unwrap_or(target))
                    .context("ZIP 目标需提供有效文件路径或十六进制头部数据")?
            };
            let verifier = parse_zip_verifier(&zip_bytes)?;
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if check_zip_password(&verifier, pw.as_bytes()) {
                    stop.store(true, Ordering::SeqCst);
                    if let Ok(mut g) = hit.lock() {
                        *g = Some((pw.clone(), None));
                    }
                }
            });
        }
        "harmony_backup" => {
            let parsed = parse_harmony_backup_target(target)?;
            words.par_iter().for_each(|pw| {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Ok(bkey) =
                    crate::core::evidence::readers::folder_image::harmony_crypto::unwrap_backup_key(
                        pw.as_bytes(),
                        &parsed.backup_salt,
                        &parsed.backup_key,
                    )
                {
                    stop.store(true, Ordering::SeqCst);
                    if let Ok(mut g) = hit.lock() {
                        let bkey_txt = String::from_utf8_lossy(&bkey);
                        let preview = match parsed.device_label.as_deref() {
                            Some(dev) => format!("device={dev}, bkey={bkey_txt}"),
                            None => format!("bkey={bkey_txt}"),
                        };
                        *g = Some((pw.clone(), Some(preview)));
                    }
                }
            });
        }
        other => bail!("不支持的破解类型: {other}"),
    }

    let matched = hit.into_inner().unwrap_or(None);
    Ok(CrackResult {
        target_type: kind,
        found: matched.is_some(),
        password: matched.as_ref().map(|m| m.0.clone()),
        decrypted_preview: matched.and_then(|m| m.1),
        tested_candidates: total,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

fn try_decrypt_fernet(token: &[u8], candidate: &str) -> Option<String> {
    let signed_len = token.len() - 32;
    let signed_part = &token[..signed_len];
    let tag = &token[signed_len..];
    let iv = &token[9..25];
    let ciphertext = &token[25..signed_len];

    let mut key_candidates: Vec<[u8; 32]> = Vec::with_capacity(3);
    // 1. Direct 32-byte base64url Fernet key
    if let Ok(decoded) = base64::engine::general_purpose::URL_SAFE
        .decode(candidate)
        .or_else(|_| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(candidate.trim_end_matches('='))
        })
    {
        if let Ok(arr) = decoded.try_into() {
            key_candidates.push(arr);
        }
    }
    // 2. SHA-256(password) -> 32-byte key (common CTF/Python pattern)
    key_candidates.push(Sha256::digest(candidate.as_bytes()).into());
    // 3. Raw 32-byte ASCII padded/truncated if exact 32 chars
    if let Ok(arr) = candidate.as_bytes().try_into() {
        key_candidates.push(arr);
    }

    for k in key_candidates {
        let signing_key = &k[0..16];
        let enc_key: [u8; 16] = k[16..32].try_into().ok()?;
        let iv_arr: [u8; 16] = iv.try_into().ok()?;
        if hmac_sha256(signing_key, signed_part) != tag {
            continue;
        }
        let Some(plain) = aes128_cbc_decrypt_pkcs7(&enc_key, &iv_arr, ciphertext) else {
            continue;
        };
        return Some(String::from_utf8_lossy(&plain).into_owned());
    }
    None
}

struct MetaMaskVault {
    ciphertext: Vec<u8>,
    iv: Vec<u8>,
    salt: Vec<u8>,
    iterations: u32,
}

fn parse_metamask_vault(json_str: &str) -> anyhow::Result<MetaMaskVault> {
    let val: serde_json::Value =
        serde_json::from_str(json_str).context("无效的 MetaMask Vault JSON")?;
    let b64 = |s: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .or_else(|_| {
                base64::engine::general_purpose::STANDARD_NO_PAD
                    .decode(s.trim().trim_end_matches('='))
            })
    };
    let data_s = val["data"]
        .as_str()
        .context("MetaMask Vault 缺少 data 字段")?;
    let iv_s = val["iv"].as_str().context("MetaMask Vault 缺少 iv 字段")?;
    let salt_s = val["salt"]
        .as_str()
        .context("MetaMask Vault 缺少 salt 字段")?;
    let ciphertext = b64(data_s).context("解码 data base64 失败")?;
    let iv = b64(iv_s).context("解码 iv base64 失败")?;
    let salt = b64(salt_s).context("解码 salt base64 失败")?;
    ensure!(ciphertext.len() > 16, "MetaMask 密文过短");
    let iterations = val
        .pointer("/keyMetadata/params/iterations")
        .and_then(serde_json::Value::as_u64)
        .map(|n| n as u32)
        .unwrap_or(10_000);
    Ok(MetaMaskVault {
        ciphertext,
        iv,
        salt,
        iterations,
    })
}

fn try_decrypt_metamask(vault: &MetaMaskVault, password: &str) -> Option<String> {
    let mut key = [0u8; 32];
    pbkdf2_hmac_sha256(password.as_bytes(), &vault.salt, vault.iterations, &mut key);
    let cipher = Aes256::new_from_slice(&key).ok()?;

    // AES-GCM CTR counter J0 for 12-byte IV is `IV || 0x00000001`; for other IV lengths GHASH is used.
    // Decrypt first block(s) with counter starting at J0 + 1 (`IV || 0x00000002`) and check JSON structure!
    if vault.iv.len() != 12 {
        return None;
    }
    let payload = &vault.ciphertext[..vault.ciphertext.len() - 16];
    let mut plain = Vec::with_capacity(payload.len());
    for (block_idx, chunk) in payload.chunks(16).enumerate() {
        let mut ctr = [0u8; 16];
        ctr[0..12].copy_from_slice(&vault.iv);
        let count = (block_idx as u32).wrapping_add(2);
        ctr[12..16].copy_from_slice(&count.to_be_bytes());
        let mut block = aes::Block::from(ctr);
        cipher.encrypt_block(&mut block);
        for (b, k) in chunk.iter().zip(block.iter()) {
            plain.push(b ^ k);
        }
    }
    let text = std::str::from_utf8(&plain).ok()?;
    let trimmed = text.trim();
    if (trimmed.starts_with('[') || trimmed.starts_with('{'))
        && (trimmed.contains("mnemonic")
            || trimmed.contains("type")
            || trimmed.contains("hd")
            || serde_json::from_str::<serde_json::Value>(trimmed).is_ok())
    {
        return Some(trimmed.to_string());
    }
    None
}

enum ZipVerifier {
    WinZipAes {
        key_len: usize,
        salt: Vec<u8>,
        pwd_verify: [u8; 2],
    },
    ZipCrypto {
        header: [u8; 12],
        check_byte: u8,
    },
}

fn parse_zip_verifier(bytes: &[u8]) -> anyhow::Result<ZipVerifier> {
    ensure!(
        bytes.len() >= 30 && &bytes[0..4] == b"PK\x03\x04",
        "不是有效的 ZIP 本地文件头 (PK\\x03\\x04)"
    );
    let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
    ensure!((flags & 0x01) != 0, "该 ZIP 条目未加密");
    let method = u16::from_le_bytes([bytes[8], bytes[9]]);
    let crc32 = u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);
    let mod_time = u16::from_le_bytes([bytes[10], bytes[11]]);
    let fname_len = u16::from_le_bytes([bytes[26], bytes[27]]) as usize;
    let extra_len = u16::from_le_bytes([bytes[28], bytes[29]]) as usize;
    let data_start = 30 + fname_len + extra_len;
    ensure!(bytes.len() >= data_start, "ZIP 文件头截断");

    if method == 99 {
        // Parse 0x9901 WinZip AES extra field
        let extra = &bytes[30 + fname_len..data_start];
        let mut pos = 0usize;
        let mut strength = 3u8;
        while pos + 4 <= extra.len() {
            let tag = u16::from_le_bytes([extra[pos], extra[pos + 1]]);
            let sz = u16::from_le_bytes([extra[pos + 2], extra[pos + 3]]) as usize;
            if tag == 0x9901 && pos + 4 + sz <= extra.len() && sz >= 7 {
                strength = extra[pos + 8];
                break;
            }
            pos += 4 + sz;
        }
        let (salt_len, key_len) = match strength {
            1 => (8usize, 16usize),
            2 => (12usize, 24usize),
            _ => (16usize, 32usize),
        };
        ensure!(
            bytes.len() >= data_start + salt_len + 2,
            "WinZip AES 盐或校验字节截断"
        );
        let salt = bytes[data_start..data_start + salt_len].to_vec();
        let pv = [
            bytes[data_start + salt_len],
            bytes[data_start + salt_len + 1],
        ];
        return Ok(ZipVerifier::WinZipAes {
            key_len,
            salt,
            pwd_verify: pv,
        });
    }

    ensure!(
        bytes.len() >= data_start + 12,
        "ZipCrypto 12 字节加密头截断"
    );
    let header: [u8; 12] = bytes[data_start..data_start + 12].try_into().unwrap();
    let check_byte = if (flags & 0x08) != 0 {
        (mod_time >> 8) as u8
    } else {
        (crc32 >> 24) as u8
    };
    Ok(ZipVerifier::ZipCrypto { header, check_byte })
}

fn check_zip_password(verifier: &ZipVerifier, pw: &[u8]) -> bool {
    match verifier {
        ZipVerifier::WinZipAes {
            key_len,
            salt,
            pwd_verify,
        } => {
            let mut derived = vec![0u8; key_len * 2 + 2];
            pbkdf2_hmac_sha1(pw, salt, 1000, &mut derived);
            &derived[key_len * 2..key_len * 2 + 2] == pwd_verify
        }
        ZipVerifier::ZipCrypto { header, check_byte } => {
            let mut k0: u32 = 0x1234_5678;
            let mut k1: u32 = 0x2345_6789;
            let mut k2: u32 = 0x3456_7890;
            let crc32_byte = |crc: u32, b: u8| -> u32 {
                let mut c = crc ^ u32::from(b);
                for _ in 0..8 {
                    c = if (c & 1) != 0 {
                        (c >> 1) ^ 0xEDB8_8320
                    } else {
                        c >> 1
                    };
                }
                c
            };
            let update_keys = |k0: &mut u32, k1: &mut u32, k2: &mut u32, b: u8| {
                *k0 = crc32_byte(*k0, b);
                *k1 = (*k1)
                    .wrapping_add(*k0 & 0xFF)
                    .wrapping_mul(134_775_813)
                    .wrapping_add(1);
                *k2 = crc32_byte(*k2, (*k1 >> 24) as u8);
            };
            for &b in pw {
                update_keys(&mut k0, &mut k1, &mut k2, b);
            }
            let mut last = 0u8;
            for &enc in header {
                let temp = (k2 | 2) as u16;
                let stream_byte = ((temp.wrapping_mul(temp ^ 1)) >> 8) as u8;
                last = enc ^ stream_byte;
                update_keys(&mut k0, &mut k1, &mut k2, last);
            }
            last == *check_byte
        }
    }
}

struct HarmonyBackupCrackTarget {
    backup_key: String,
    backup_salt: String,
    device_label: Option<String>,
}

fn parse_harmony_backup_target(target: &str) -> anyhow::Result<HarmonyBackupCrackTarget> {
    let trimmed = target.trim();
    let raw_text = if trimmed.starts_with('{') {
        trimmed.to_owned()
    } else {
        let p = std::path::Path::new(trimmed);
        let info_path = if p.is_dir() {
            p.join(".info.json")
        } else {
            p.to_path_buf()
        };
        std::fs::read_to_string(&info_path).with_context(|| {
            format!(
                "读取 HarmonyOS 备份 .info.json 失败: {}",
                info_path.display()
            )
        })?
    };
    let val: serde_json::Value =
        serde_json::from_str(&raw_text).context("无效的 HarmonyOS .info.json JSON")?;
    let enc_type = val
        .pointer("/BackupFileTypeInfo/encryptionType")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    ensure!(
        enc_type != 0,
        "该 HarmonyOS 备份未加密 (encryptionType == 0)"
    );
    let backup_key = val
        .pointer("/BackupFileTypeInfo/backupKey")
        .and_then(serde_json::Value::as_str)
        .context("缺少 BackupFileTypeInfo.backupKey")?
        .to_owned();
    let backup_salt = val
        .pointer("/BackupFileTypeInfo/backupSalt")
        .and_then(serde_json::Value::as_str)
        .context("缺少 BackupFileTypeInfo.backupSalt")?
        .to_owned();
    let device_label = val
        .pointer("/BackupFileHeaderInfo/productModel")
        .or_else(|| val.pointer("/BackupFileHeaderInfo/deviceModel"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    Ok(HarmonyBackupCrackTarget {
        backup_key,
        backup_salt,
        device_label,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cracks_ntlm_hash_with_rules() {
        let target_hash = hex::encode(ntlm_hash("Lovely2026"));
        let req = CrackRequest {
            target_type: "ntlm".into(),
            target: target_hash,
            wordlist_path: None,
            candidates: vec!["lovely".into()],
            enable_rules: Some(true),
            pin_length: None,
        };
        let res = run_crack(&req).unwrap();
        assert!(res.found);
        assert_eq!(res.password.as_deref(), Some("Lovely2026"));
    }

    #[test]
    fn cracks_fernet_token_and_returns_plaintext() {
        let pw = "secret123";
        let full_key: [u8; 32] = Sha256::digest(pw.as_bytes()).into();
        let sign_key = &full_key[0..16];
        let enc_key: [u8; 16] = full_key[16..32].try_into().unwrap();
        let iv = [0x42u8; 16];
        let plaintext = b"flag{fernet_cracked}";
        let pad_len = 16 - (plaintext.len() % 16);
        let mut padded = plaintext.to_vec();
        padded.extend(std::iter::repeat_n(pad_len as u8, pad_len));
        let cipher = Aes128::new_from_slice(&enc_key).unwrap();
        let mut ct = Vec::new();
        let mut prev = iv;
        for chunk in padded.chunks_exact(16) {
            let mut xored = [0u8; 16];
            for i in 0..16 {
                xored[i] = chunk[i] ^ prev[i];
            }
            let mut blk = aes::Block::from(xored);
            cipher.encrypt_block(&mut blk);
            prev.copy_from_slice(&blk);
            ct.extend_from_slice(&blk);
        }

        let mut token = vec![0x80u8];
        token.extend_from_slice(&1_700_000_000i64.to_be_bytes());
        token.extend_from_slice(&iv);
        token.extend_from_slice(&ct);
        let mac = hmac_sha256(sign_key, &token);
        token.extend_from_slice(&mac);
        let token_b64 = base64::engine::general_purpose::URL_SAFE.encode(&token);

        let req = CrackRequest {
            target_type: "fernet".into(),
            target: token_b64,
            wordlist_path: None,
            candidates: vec!["secret".into()],
            enable_rules: Some(true),
            pin_length: None,
        };
        let res = run_crack(&req).unwrap();
        assert!(res.found);
        assert_eq!(res.password.as_deref(), Some("secret123"));
        assert_eq!(
            res.decrypted_preview.as_deref(),
            Some("flag{fernet_cracked}")
        );
    }

    #[test]
    fn cracks_harmony_next_backup_info_json() {
        use crate::core::evidence::readers::folder_image::harmony_crypto::{
            compute_j0, hmac_sha256, pbkdf2_hmac_sha256,
        };
        use aes::cipher::{BlockCipherEncrypt, KeyInit};

        let pw = "P9x#rG1t6h";
        let salt = [0x11u8; 32];
        let iv = [0x22u8; 16];
        let bkey = b"g70SV5/lvfcUdEMkQT79nA==";
        let mut kek = [0u8; 32];
        pbkdf2_hmac_sha256(pw.as_bytes(), &salt, 10_000, &mut kek);

        // Encrypt using AES-GCM with 16-byte nonce for self-contained cracker test.
        let cipher = Aes256::new_from_slice(&kek).unwrap();
        let j0 = compute_j0(&kek, &iv).unwrap();
        let mut h_blk = aes::Block::from([0u8; 16]);
        cipher.encrypt_block(&mut h_blk);
        let h = u128::from_be_bytes(h_blk.into());

        let mut ct = bkey.to_vec();
        for (idx, chunk) in ct.chunks_mut(16).enumerate() {
            let mut ctr = j0;
            let low = u32::from_be_bytes([ctr[12], ctr[13], ctr[14], ctr[15]])
                .wrapping_add(1 + idx as u32);
            ctr[12..16].copy_from_slice(&low.to_be_bytes());
            let mut ks = aes::Block::from(ctr);
            cipher.encrypt_block(&mut ks);
            for (dst, &k) in chunk.iter_mut().zip(ks.iter()) {
                *dst ^= k;
            }
        }
        let gf_mul = |x: u128, y: u128| -> u128 {
            let mut z = 0u128;
            let mut v = x;
            for i in 0..128 {
                if ((y >> (127 - i)) & 1) == 1 {
                    z ^= v;
                }
                let lsb = v & 1;
                v >>= 1;
                if lsb == 1 {
                    v ^= 0xe100_0000_0000_0000_0000_0000_0000_0000u128;
                }
            }
            z
        };
        let mut y = 0u128;
        for chunk in ct.chunks(16) {
            let mut blk = [0u8; 16];
            blk[..chunk.len()].copy_from_slice(chunk);
            y = gf_mul(y ^ u128::from_be_bytes(blk), h);
        }
        let mut len_blk = [0u8; 16];
        len_blk[8..16].copy_from_slice(&((ct.len() as u64) * 8).to_be_bytes());
        y = gf_mul(y ^ u128::from_be_bytes(len_blk), h);
        let mut e_j0 = aes::Block::from(j0);
        cipher.encrypt_block(&mut e_j0);
        let tag = (y ^ u128::from_be_bytes(e_j0.into())).to_be_bytes();

        let _ = hmac_sha256(b"k", b"d");
        let mut bk_blob = vec![0u8; 12];
        bk_blob.extend_from_slice(&iv);
        bk_blob.extend_from_slice(&ct);
        bk_blob.extend_from_slice(&tag);

        let info_json = serde_json::json!({
            "BackupFileHeaderInfo": { "productModel": "VYG-AL30" },
            "BackupFileTypeInfo": {
                "encryptionType": 2,
                "backupKey": base64::engine::general_purpose::STANDARD.encode(&bk_blob),
                "backupSalt": base64::engine::general_purpose::STANDARD.encode(salt),
                "backupIv": base64::engine::general_purpose::STANDARD.encode(iv),
            }
        });
        let req = CrackRequest {
            target_type: "harmony_backup".into(),
            target: info_json.to_string(),
            wordlist_path: None,
            candidates: vec!["wrong".into(), pw.into()],
            enable_rules: Some(false),
            pin_length: None,
        };
        let res = run_crack(&req).unwrap();
        assert!(res.found);
        assert_eq!(res.password.as_deref(), Some(pw));
        assert!(res.decrypted_preview.unwrap().contains("VYG-AL30"));
    }
}
