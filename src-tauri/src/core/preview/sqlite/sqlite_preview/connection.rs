//! connection implementation.
use super::*;
use crate::core::common::error::Failure;

/// Open a VFS-backed preview session read-only, transparently applying
/// SQLCipher key/params when the file is not a plaintext SQLite database.
/// Returns the connection plus whether the database was encrypted.
///
/// Detection is by header: a plaintext database starts with
/// `"SQLite format 3\0"`; an encrypted one does not (its header is
/// ciphertext). For an encrypted file we apply `key` then run a cheap
/// schema probe — a wrong or missing key trips `SQLITE_NOTADB` here, and
/// we surface it as a `SQLITE_LOCKED:` error the frontend recognises to
/// prompt for a key (rather than a confusing "file is not a database").
pub(super) fn open_session_conn(
    session: &SqliteSession,
    key: &SqliteKey,
) -> anyhow::Result<(Connection, bool)> {
    open_conn_with_head(session.head16, key, || {
        sqlite_vfs::open_readonly(&session.main_name)
    })
}

#[cfg(test)]
pub(super) fn open_path_conn(path: &Path, key: &SqliteKey) -> anyhow::Result<(Connection, bool)> {
    open_conn_with_head(read_head16(path), key, || {
        Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )
    })
}

pub(super) fn open_conn_with_head<F>(
    head: Option<[u8; 16]>,
    key: &SqliteKey,
    open: F,
) -> anyhow::Result<(Connection, bool)>
where
    F: FnOnce() -> rusqlite::Result<Connection>,
{
    // Too short to be any SQLite/SQLCipher database → "not a database",
    // which is a different (and more honest) message than "encrypted,
    // provide a key". Catches 0-byte / truncated fragments.
    let Some(head) = head else {
        anyhow::bail!("不是 SQLite 数据库（文件过小或不可读）");
    };
    let plain_magic = looks_like_sqlite(&head);

    let conn = open()?;

    // Apply a key when the header isn't plaintext magic, OR when the
    // caller supplied key material anyway — the latter covers
    // `cipher_plaintext_header_size` databases, whose page-1 header (incl.
    // the magic) stays cleartext while the page bodies are encrypted.
    let attempted_decrypt = !plain_magic || key.has_material();
    if attempted_decrypt {
        apply_key(&conn, key)?;
    }

    // Touch the schema so a bad key fails here, deterministically, rather
    // than on the first table read deeper in the call.
    if conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .is_err()
    {
        if plain_magic && !attempted_decrypt {
            // Plaintext header but no key tried: genuinely corrupt, or a
            // plaintext-header encrypted DB awaiting a key. Offer the key
            // prompt either way, with an honest hint. (Note: we never
            // include the underlying SQLite error text — on the key paths
            // it can embed the PRAGMA statement, hence the raw key.)
            anyhow::bail!(
                Failure::corrupt("SQLITE_LOCKED: 无法读取数据库架构（可能已损坏，或为保留明文头的加密库，请尝试提供密钥）")
            );
        }
        let detail = if key.has_material() {
            "密钥或兼容参数不正确"
        } else {
            "数据库已加密，请提供密钥"
        };
        anyhow::bail!("SQLITE_LOCKED: {detail}");
    }
    register_forensic_functions(&conn)?;
    Ok((conn, attempted_decrypt))
}

fn format_utc8_secs(secs: i64, nanos: u32) -> Option<String> {
    use chrono::{FixedOffset, TimeZone};
    let tz = FixedOffset::east_opt(8 * 3600)?;
    let dt = tz.timestamp_opt(secs, nanos).single()?;
    if nanos > 0 {
        Some(dt.format("%Y-%m-%d %H:%M:%S%.3f UTC+08").to_string())
    } else {
        Some(dt.format("%Y-%m-%d %H:%M:%S UTC+08").to_string())
    }
}

fn parse_numeric_arg(ctx: &rusqlite::functions::Context<'_>) -> Option<f64> {
    match ctx.get_raw(0) {
        ValueRef::Integer(i) => Some(i as f64),
        ValueRef::Real(f) => Some(f),
        ValueRef::Text(t) => std::str::from_utf8(t).ok()?.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Register forensic scalar SQL functions (`LC_UNIX`, `LC_UNIX_MS`,
/// `LC_WEBKIT`, `LC_COCOA`, `LC_FILETIME`, `LC_PROTO`) on `conn`.
pub(super) fn register_forensic_functions(conn: &Connection) -> anyhow::Result<()> {
    use rusqlite::functions::FunctionFlags;
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;

    conn.create_scalar_function("LC_UNIX", 1, flags, |ctx| {
        let Some(v) = parse_numeric_arg(ctx) else {
            return Ok(None::<String>);
        };
        let secs = v.trunc() as i64;
        let nanos = ((v.fract().abs()) * 1_000_000_000.0) as u32;
        Ok(format_utc8_secs(secs, nanos))
    })?;

    conn.create_scalar_function("LC_UNIX_MS", 1, flags, |ctx| {
        let Some(v) = parse_numeric_arg(ctx) else {
            return Ok(None::<String>);
        };
        let ms = v as i64;
        let secs = ms.div_euclid(1_000);
        let nanos = (ms.rem_euclid(1_000) as u32) * 1_000_000;
        Ok(format_utc8_secs(secs, nanos))
    })?;

    conn.create_scalar_function("LC_WEBKIT", 1, flags, |ctx| {
        let Some(v) = parse_numeric_arg(ctx) else {
            return Ok(None::<String>);
        };
        let us = v as i64;
        if us <= 0 {
            return Ok(None::<String>);
        }
        let unix_us = us - 11_644_473_600_000_000;
        let secs = unix_us.div_euclid(1_000_000);
        let nanos = (unix_us.rem_euclid(1_000_000) as u32) * 1_000;
        Ok(format_utc8_secs(secs, nanos))
    })?;

    conn.create_scalar_function("LC_COCOA", 1, flags, |ctx| {
        let Some(v) = parse_numeric_arg(ctx) else {
            return Ok(None::<String>);
        };
        // Auto-handle both seconds and nanoseconds since 2001-01-01.
        let secs_f = if v.abs() > 1e14 {
            v / 1_000_000_000.0
        } else {
            v
        };
        let unix_secs = (secs_f.trunc() as i64) + 978_307_200;
        let nanos = ((secs_f.fract().abs()) * 1_000_000_000.0) as u32;
        Ok(format_utc8_secs(unix_secs, nanos))
    })?;

    conn.create_scalar_function("LC_FILETIME", 1, flags, |ctx| {
        let Some(v) = parse_numeric_arg(ctx) else {
            return Ok(None::<String>);
        };
        let ticks = v as i64;
        if ticks <= 0 {
            return Ok(None::<String>);
        }
        let unix_100ns = ticks - 116_444_736_000_000_000;
        let secs = unix_100ns.div_euclid(10_000_000);
        let nanos = (unix_100ns.rem_euclid(10_000_000) as u32) * 100;
        Ok(format_utc8_secs(secs, nanos))
    })?;

    conn.create_scalar_function("LC_PROTO", 1, flags, |ctx| {
        let bytes: Option<Vec<u8>> = match ctx.get_raw(0) {
            ValueRef::Blob(b) => Some(b.to_vec()),
            ValueRef::Text(t) => {
                let s = std::str::from_utf8(t).unwrap_or("");
                hex::decode(s.trim().strip_prefix("0x").unwrap_or(s.trim())).ok()
            }
            _ => None,
        };
        let Some(data) = bytes else {
            return Ok(None::<String>);
        };
        Ok(crate::core::preview::sqlite::blob_decode::decode_blob(&data).map(|d| d.summary))
    })?;

    Ok(())
}

/// Apply key material and cipher parameters to a freshly opened
/// connection. Order matters: `key` first, then `cipher_compatibility`
/// (which resets the parameter set for a SQLCipher major version),
/// then the individual `cipher_page_size` / `kdf_iter` overrides.
///
/// Errors from the key-bearing PRAGMAs are deliberately *replaced* with a
/// generic message (never chained): a raw `rusqlite` error can embed the
/// offending SQL statement — i.e. the `x'…'` raw key — which must not
/// reach the UI or logs.
pub(super) fn apply_key(conn: &Connection, key: &SqliteKey) -> anyhow::Result<()> {
    if let Some(raw) = key
        .raw_hex_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        // Raw 256-bit key (optionally key+salt). Must be applied as a
        // hex blob literal `x'…'`, not a quoted string, so we build the
        // PRAGMA by hand — after validating the hex to keep it injection-safe.
        let cleaned: String = raw
            .trim_prefix("0x")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if cleaned.is_empty()
            || !cleaned.len().is_multiple_of(2)
            || !cleaned.bytes().all(|b| b.is_ascii_hexdigit())
        {
            anyhow::bail!(
                "原始密钥必须是偶数长度的十六进制字符串（例如 32 字节为 64 个十六进制字符）"
            );
        }
        conn.execute_batch(&format!("PRAGMA key = \"x'{cleaned}'\";"))
            .map_err(|_| anyhow::anyhow!("应用原始密钥失败（格式或长度不被接受）"))?;
    } else if let Some(pass) = key.passphrase.as_deref().filter(|s| !s.is_empty()) {
        // rusqlite quotes/escapes the value as a string literal — the
        // exact form SQLCipher expects for a passphrase. Drop the source
        // error too: pragma_update inlines the value into the statement.
        conn.pragma_update(None, "key", pass)
            .map_err(|_| anyhow::anyhow!("应用口令失败"))?;
    }

    if let Some(compat) = key.compatibility {
        if (1..=4).contains(&compat) {
            conn.execute_batch(&format!("PRAGMA cipher_compatibility = {compat};"))?;
        }
    }
    if let Some(page_size) = key.page_size {
        conn.execute_batch(&format!("PRAGMA cipher_page_size = {page_size};"))?;
    }
    if let Some(kdf_iter) = key.kdf_iter {
        conn.execute_batch(&format!("PRAGMA kdf_iter = {kdf_iter};"))?;
    }
    Ok(())
}

/// Read the first 16 bytes of `path`, or `None` if the file is shorter
/// than that (or unreadable). `Some` + [`looks_like_sqlite`] ⇒ plaintext
/// SQLite header; `None` ⇒ too small to be any database.
#[cfg(test)]
pub(super) fn read_head16(path: &Path) -> Option<[u8; 16]> {
    let mut buf = [0u8; 16];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut buf))
        .ok()?;
    Some(buf)
}
