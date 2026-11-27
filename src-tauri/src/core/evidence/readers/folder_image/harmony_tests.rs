use std::fs;
use std::path::Path;

use base64::Engine;
use tokio_util::sync::CancellationToken;

use super::harmony::probe_info_json;
use super::harmony_crypto::{
    aes256_gcm_ctr_read_at, aes256_gcm_decrypt_and_verify, aes256_gcm_encrypt_for_test, compute_j0,
    pbkdf2_hmac_sha256, unwrap_backup_key,
};
use super::FolderImage;

fn make_tar(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        for &(name, data) in members {
            let mut hdr = tar::Header::new_ustar();
            hdr.set_size(data.len() as u64);
            hdr.set_entry_type(tar::EntryType::Regular);
            hdr.set_mode(0o644);
            hdr.set_mtime(1_794_899_970);
            hdr.set_cksum();
            builder.append_data(&mut hdr, name, data).unwrap();
        }
        builder.finish().unwrap();
    }
    tar_bytes
}

#[test]
fn harmony_crypto_random_seek_ctr_matches_full_gcm_decrypt() {
    let password = b"P9x#rG1t6h";
    let b_salt = [0x11u8; 32];
    let b_iv = [0x22u8; 16]; // 16-byte nonce -> GHASH J0 path
    let bkey_ascii = b"g70SV5/lvfcUdEMkQT79nA==";
    let mut kek = [0u8; 32];
    pbkdf2_hmac_sha256(password, &b_salt, 10_000, &mut kek);
    let ct_bk = aes256_gcm_encrypt_for_test(&kek, &b_iv, bkey_ascii);
    let mut bk_blob = Vec::from([0u8; 12]);
    bk_blob.extend_from_slice(&b_iv);
    bk_blob.extend_from_slice(&ct_bk[..bkey_ascii.len()]);
    bk_blob.extend_from_slice(&ct_bk[bkey_ascii.len()..]);

    let unwrapped = unwrap_backup_key(
        password,
        &base64::engine::general_purpose::STANDARD.encode(b_salt),
        &base64::engine::general_purpose::STANDARD.encode(&bk_blob),
    )
    .unwrap();
    assert_eq!(unwrapped, bkey_ascii);

    // Wrong password rejected by tag verification
    assert!(unwrap_backup_key(
        b"wrong_pw",
        &base64::engine::general_purpose::STANDARD.encode(b_salt),
        &base64::engine::general_purpose::STANDARD.encode(&bk_blob),
    )
    .is_err());

    // O(1) CTR seek at arbitrary unaligned offset across 16B blocks
    let mod_key = [0x55u8; 32];
    let mod_iv = [0x77u8; 16];
    let payload: Vec<u8> = (0..65_536u32)
        .map(|i| (i.wrapping_mul(31) & 0xff) as u8)
        .collect();
    let enc = aes256_gcm_encrypt_for_test(&mod_key, &mod_iv, &payload);
    assert_eq!(
        aes256_gcm_decrypt_and_verify(&mod_key, &mod_iv, &enc).unwrap(),
        payload
    );

    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("blob.bin");
    fs::write(&p, &enc).unwrap();
    let j0 = compute_j0(&mod_key, &mod_iv).unwrap();
    let slice = aes256_gcm_ctr_read_at(
        &mod_key,
        &j0,
        &p,
        512, // simulated base_plain_offset (e.g. inside TAR member)
        payload.len() as u64 - 512,
        12_345,
        777,
    )
    .unwrap();
    assert_eq!(slice, payload[512 + 12_345..512 + 12_345 + 777]);
}

fn build_synthetic_harmony_backup(root: &Path, encrypted: bool) {
    let password = b"P9x#rG1t6h";
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);

    let b_salt = [0x11u8; 32];
    let b_iv = [0x22u8; 16];
    let bkey_ascii = b"g70SV5/lvfcUdEMkQT79nA==";
    let m_salt = [0x33u8; 32];
    let m_iv = [0x44u8; 16];
    let sys_key = [0x66u8; 32];
    let sys_iv = [0x77u8; 32]; // 32B IV + comma decimal format (`WLAN_SA`)

    let mut kek = [0u8; 32];
    pbkdf2_hmac_sha256(password, &b_salt, 10_000, &mut kek);
    let enc_bkey = aes256_gcm_encrypt_for_test(&kek, &b_iv, bkey_ascii);
    let mut bk_blob = vec![0u8; 12];
    bk_blob.extend_from_slice(&b_iv);
    bk_blob.extend_from_slice(&enc_bkey);

    let mut mod_key = [0u8; 32];
    pbkdf2_hmac_sha256(bkey_ascii, &m_salt, 10_000, &mut mod_key);

    let enc_or_raw = |key: &[u8; 32], iv: &[u8], data: &[u8]| -> Vec<u8> {
        if encrypted {
            aes256_gcm_encrypt_for_test(key, iv, data)
        } else {
            data.to_vec()
        }
    };

    let comma = |b: &[u8]| b.iter().map(u8::to_string).collect::<Vec<_>>().join(",");

    let info_json = serde_json::json!({
        "BackupFileHeaderInfo": {
            "backupVersion": 1,
            "deviceModel": "Mate 80",
            "productModel": "VYG-AL30",
            "deviceId": "default",
            "selectDataSize": 4096,
            "backupTime": 1794899961932i64,
            "specialBundles": ["contact", "calllog"]
        },
        "BackupFileVersionInfo": {
            "versionCode": "70000525",
            "versionName": "7.0.0.525"
        },
        "BackupFileTypeInfo": {
            "encryptionType": if encrypted { 2 } else { 0 },
            "backupKey": b64(&bk_blob),
            "backupSalt": b64(&b_salt),
            "backupIv": b64(&b_iv),
            "backupReminder": "pwd-hint"
        },
        "BackupFileModuleInfo": [
            {
                "moduleName": "com.example.chat",
                "module_name": "com.example.chat:1",
                "module_type": "NEXT_APP",
                "appName": "示例聊天",
                "appIndex": 1,
                "encryptionSalt": b64(&m_salt),
                "encryptionIv": b64(&m_iv)
            },
            {
                "moduleName": "1120",
                "module_name": "WLAN_SA",
                "module_type": "SYSTEM",
                "appName": "1120",
                "appIndex": 0,
                "encryptionSalt": comma(&sys_key),
                "encryptionIv": comma(&sys_iv)
            }
        ]
    });
    fs::write(
        root.join(".info.json"),
        serde_json::to_vec(&info_json).unwrap(),
    )
    .unwrap();

    // Clone module: files/APPLICATION/com.example.chat_1 (note NEXT_APP -> APPLICATION on disk!)
    let wc_dir = root
        .join("files")
        .join("APPLICATION")
        .join("com.example.chat_1");
    fs::create_dir_all(&wc_dir).unwrap();
    let long_payload = vec![b'Z'; 4096];
    let tar_bytes = make_tar(&[
        (
            "data/storage/el2/base/files/users/u1/hello.txt",
            b"harmony-tar-hello",
        ),
        (
            "data/storage/el2/base/files/users/u1/bigslice.bin",
            &long_payload,
        ),
    ]);
    fs::write(
        wc_dir.join("part.0.tar"),
        enc_or_raw(&mod_key, &m_iv, &tar_bytes),
    )
    .unwrap();
    fs::write(
        wc_dir.join("abcdef1234567890"),
        enc_or_raw(&mod_key, &m_iv, b"bigfile-content-123"),
    )
    .unwrap();
    fs::write(
        wc_dir.join("com.example.chat.png"),
        b"\x89PNG\r\n\x1a\nICON",
    )
    .unwrap();
    let manage = serde_json::json!([
        {
            "fileName": "part.0.tar",
            "isBigFile": false,
            "information": { "path": "/data/storage/el2/base/.backup/backup/part.0.tar" }
        },
        {
            "fileName": "abcdef1234567890",
            "isBigFile": true,
            "information": {
                "path": "/data/storage/el2/base/big/note.txt",
                "stat": { "st_size": 19, "st_mtim": { "tv_sec": 1794899985 } }
            }
        }
    ]);
    fs::write(
        wc_dir.join("manage.json"),
        enc_or_raw(&mod_key, &m_iv, &serde_json::to_vec(&manage).unwrap()),
    )
    .unwrap();

    // SYSTEM module 1120 (no manage.json, comma-byte 32B symkey + 32B iv)
    let sys_dir = root.join("files").join("SYSTEM").join("1120");
    fs::create_dir_all(&sys_dir).unwrap();
    fs::write(
        sys_dir.join("1120_sa.data"),
        enc_or_raw(&sys_key, &sys_iv, b"[WifiBackupConfig_1] ok"),
    )
    .unwrap();
}

#[test]
fn detects_unlocks_and_reads_encrypted_and_plain_harmony_backups() {
    let tmp = tempfile::tempdir().unwrap();
    let enc_root = tmp.path().join("enc_backup");
    fs::create_dir_all(&enc_root).unwrap();
    build_synthetic_harmony_backup(&enc_root, true);

    assert!(
        probe_info_json(&fs::read(enc_root.join(".info.json")).unwrap())
            .is_some_and(|i| i.locked && i.clone_modules == vec!["com.example.chat:1"])
    );

    let locked_img = FolderImage::open(&enc_root).unwrap();
    let linfo = locked_img.harmony_info().unwrap();
    assert!(linfo.locked);
    assert_eq!(linfo.module_count, 2);
    assert_eq!(linfo.device_model.as_deref(), Some("Mate 80"));
    assert_eq!(
        linfo.backup_time_utc8.as_deref(),
        Some("2026-11-17 15:19:21")
    );
    assert_eq!(linfo.backup_reminder.as_deref(), Some("pwd-hint"));

    assert!(FolderImage::open_harmony(&enc_root, Some("bad"), &CancellationToken::new()).is_err());

    let unlocked =
        FolderImage::open_harmony(&enc_root, Some("P9x#rG1t6h"), &CancellationToken::new())
            .unwrap();
    assert!(!unlocked.harmony_info().unwrap().locked);
    assert_eq!(
        unlocked
            .read(
                "modules/com.example.chat_1/data/storage/el2/base/files/users/u1/hello.txt",
                1024
            )
            .unwrap(),
        b"harmony-tar-hello"
    );
    assert_eq!(
        unlocked
            .read_harmony_range(
                "modules/com.example.chat_1/data/storage/el2/base/files/users/u1/bigslice.bin",
                123,
                10
            )
            .unwrap()
            .unwrap(),
        vec![b'Z'; 10]
    );
    assert_eq!(
        unlocked
            .read(
                "modules/com.example.chat_1/data/storage/el2/base/big/note.txt",
                1024
            )
            .unwrap(),
        b"bigfile-content-123"
    );
    assert_eq!(
        unlocked
            .read("modules/com.example.chat_1/com.example.chat.png", 1024)
            .unwrap(),
        b"\x89PNG\r\n\x1a\nICON"
    );
    assert_eq!(
        unlocked.read("modules/1120/1120_sa.data", 1024).unwrap(),
        b"[WifiBackupConfig_1] ok"
    );

    // Unencrypted (encryptionType = 0) opens directly unlocked.
    let plain_root = tmp.path().join("plain_backup");
    fs::create_dir_all(&plain_root).unwrap();
    build_synthetic_harmony_backup(&plain_root, false);
    let plain_img = FolderImage::open(&plain_root).unwrap();
    assert!(!plain_img.harmony_info().unwrap().locked);
    assert_eq!(
        plain_img
            .read(
                "modules/com.example.chat_1/data/storage/el2/base/big/note.txt",
                1024
            )
            .unwrap(),
        b"bigfile-content-123"
    );
}

#[test]
fn harmony_tar_ignores_nonzero_directory_size_header() {
    let mut raw_tar = Vec::new();
    // Directory entry with non-zero st_size (3440) as emitted by HarmonyOS F2FS backup tar:
    // must NOT advance TAR offset past the directory header.
    let mut dir_hdr = tar::Header::new_ustar();
    dir_hdr.set_entry_type(tar::EntryType::Directory);
    dir_hdr.set_size(3440);
    dir_hdr.set_mode(0o755);
    dir_hdr
        .set_path("data/storage/el2/base/haps/entry/cache")
        .unwrap();
    dir_hdr.set_cksum();
    raw_tar.extend_from_slice(dir_hdr.as_bytes());

    let fsend_bytes = b"SQLite format 3\0fsend-payload-exact";
    let mut file_hdr = tar::Header::new_ustar();
    file_hdr.set_entry_type(tar::EntryType::Regular);
    file_hdr.set_size(fsend_bytes.len() as u64);
    file_hdr.set_mode(0o644);
    file_hdr
        .set_path("data/storage/el2/database/entry/rdb/FSend.db")
        .unwrap();
    file_hdr.set_cksum();
    raw_tar.extend_from_slice(file_hdr.as_bytes());
    raw_tar.extend_from_slice(fsend_bytes);
    let rem = fsend_bytes.len() % 512;
    if rem != 0 {
        raw_tar.resize(raw_tar.len() + (512 - rem), 0);
    }
    raw_tar.resize(raw_tar.len() + 1024, 0);

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let m_iv = [0x44u8; 16];
    let mod_key = [0x99u8; 32];
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);

    let info_json = serde_json::json!({
        "BackupFileVersionInfo": { "versionName": "7.0.0.525", "versionCode": "70000525" },
        "BackupFileHeaderInfo": { "backupVersion": 1, "deviceModel": "Mate 80", "productModel": "VYG-AL30" },
        "BackupFileTypeInfo": { "encryptionType": 0 },
        "BackupFileModuleInfo": [
            {
                "moduleName": "com.xiaolifaa.fastfiletransfer",
                "module_name": "com.xiaolifaa.fastfiletransfer",
                "module_type": "APPLICATION",
                "appIndex": 0,
                "encryptionSalt": b64(&mod_key),
                "encryptionIv": b64(&m_iv)
            }
        ]
    });
    fs::write(
        root.join(".info.json"),
        serde_json::to_vec(&info_json).unwrap(),
    )
    .unwrap();
    let app_dir = root
        .join("files")
        .join("APPLICATION")
        .join("com.xiaolifaa.fastfiletransfer");
    fs::create_dir_all(&app_dir).unwrap();
    fs::write(app_dir.join("part.0.tar"), &raw_tar).unwrap();
    let manage = serde_json::json!([
        { "fileName": "part.0.tar", "isBigFile": false, "information": { "path": "/data/storage/el2/base/.backup/backup/part.0.tar" } }
    ]);
    fs::write(
        app_dir.join("manage.json"),
        serde_json::to_vec(&manage).unwrap(),
    )
    .unwrap();

    let img = FolderImage::open(root).unwrap();
    let got = img
        .read(
            "modules/com.xiaolifaa.fastfiletransfer/data/storage/el2/database/entry/rdb/FSend.db",
            4096,
        )
        .unwrap();
    assert_eq!(got, fsend_bytes);
}
