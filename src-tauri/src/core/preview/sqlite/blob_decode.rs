//! Forensic BLOB auto-decoder for SQLite cells and LevelDB values:
//! transparently unwraps compression layers (`zlib`, `gzip`, `zstd`, `lz4`)
//! and decodes schema-less `Protobuf`, `bplist00` (`NSKeyedArchiver`), `JSON`,
//! and `MessagePack` payloads.

use std::collections::HashSet;
use std::io::{Cursor, Read};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

const MAX_DECOMPRESSED_BYTES: usize = 2 * 1024 * 1024;
const MAX_SUMMARY_CHARS: usize = 160;
const MAX_PROTO_DEPTH: usize = 8;
const MAX_PLIST_DEPTH: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecodedBlob {
    pub format: String,
    pub summary: String,
    pub detail: Value,
}

/// Inspect `bytes` and return a structured forensic decode when `bytes` matches
/// a recognized compressed or structured serialization format.
#[must_use]
pub fn decode_blob(bytes: &[u8]) -> Option<DecodedBlob> {
    if bytes.len() < 2 {
        return None;
    }

    let mut chain: Vec<&'static str> = Vec::new();
    let mut current: Vec<u8> = bytes.to_vec();

    // Unwrap up to 4 nested compression envelopes safely.
    for _ in 0..4 {
        if let Some((tag, decompressed)) = try_decompress(&current) {
            chain.push(tag);
            current = decompressed;
        } else {
            break;
        }
    }

    let (payload_fmt, detail) = decode_structured_payload(&current)?;
    chain.push(payload_fmt);
    let format = chain.join("+");
    let detail = truncate_json_value(detail, 0);
    let summary = compact_summary(&format, &detail);
    Some(DecodedBlob {
        format,
        summary,
        detail,
    })
}

/// Try decompressing `zlib`, `gzip`, `zstd`, or `lz4` frames/blocks within
/// [`MAX_DECOMPRESSED_BYTES`].
fn try_decompress(bytes: &[u8]) -> Option<(&'static str, Vec<u8>)> {
    if bytes.len() >= 2
        && bytes[0] == 0x78
        && u16::from_be_bytes([bytes[0], bytes[1]]).is_multiple_of(31)
    {
        let mut decoder =
            flate2::read::ZlibDecoder::new(bytes).take(MAX_DECOMPRESSED_BYTES as u64 + 1);
        let mut out = Vec::new();
        if decoder.read_to_end(&mut out).is_ok()
            && !out.is_empty()
            && out.len() <= MAX_DECOMPRESSED_BYTES
        {
            return Some(("zlib", out));
        }
    }
    if bytes.len() >= 10 && bytes.starts_with(&[0x1f, 0x8b, 0x08]) {
        let mut decoder =
            flate2::read::GzDecoder::new(bytes).take(MAX_DECOMPRESSED_BYTES as u64 + 1);
        let mut out = Vec::new();
        if decoder.read_to_end(&mut out).is_ok()
            && !out.is_empty()
            && out.len() <= MAX_DECOMPRESSED_BYTES
        {
            return Some(("gzip", out));
        }
    }
    if bytes.len() >= 4 && bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        if let Ok(dec) = zstd::stream::Decoder::new(bytes) {
            let mut out = Vec::new();
            if dec
                .take(MAX_DECOMPRESSED_BYTES as u64 + 1)
                .read_to_end(&mut out)
                .is_ok()
                && !out.is_empty()
                && out.len() <= MAX_DECOMPRESSED_BYTES
            {
                return Some(("zstd", out));
            }
        }
    }
    if bytes.len() >= 7 && bytes.starts_with(&[0x04, 0x22, 0x4d, 0x18]) {
        if let Some(out) = decompress_lz4_frame(bytes) {
            return Some(("lz4", out));
        }
    }
    None
}

fn decompress_lz4_frame(bytes: &[u8]) -> Option<Vec<u8>> {
    // Minimal LZ4 frame decoder supporting standard FLG/BD/optional content-size + data blocks.
    let flg = *bytes.get(4)?;
    let version = (flg >> 6) & 0x03;
    if version != 0x01 {
        return None;
    }
    let has_block_checksum = (flg & 0x10) != 0;
    let has_content_size = (flg & 0x08) != 0;
    let has_dict_id = (flg & 0x01) != 0;
    let mut pos = 6usize;
    if has_content_size {
        pos = pos.checked_add(8)?;
    }
    if has_dict_id {
        pos = pos.checked_add(4)?;
    }
    pos = pos.checked_add(1)?; // header checksum byte
    let mut out = Vec::new();
    while pos + 4 <= bytes.len() {
        let raw_size =
            u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
        pos += 4;
        if raw_size == 0 {
            return (!out.is_empty()).then_some(out);
        }
        let uncompressed = (raw_size & 0x8000_0000) != 0;
        let block_len = (raw_size & 0x7FFF_FFFF) as usize;
        let block = bytes.get(pos..pos + block_len)?;
        pos += block_len;
        if has_block_checksum {
            pos = pos.checked_add(4)?;
        }
        if uncompressed {
            if out.len() + block.len() > MAX_DECOMPRESSED_BYTES {
                return None;
            }
            out.extend_from_slice(block);
        } else {
            decompress_lz4_block(block, &mut out)?;
        }
    }
    (!out.is_empty()).then_some(out)
}

fn decompress_lz4_block(src: &[u8], out: &mut Vec<u8>) -> Option<()> {
    let mut pos = 0usize;
    while pos < src.len() {
        let token = src[pos];
        pos += 1;
        let mut lit_len = (token >> 4) as usize;
        if lit_len == 15 {
            loop {
                let extra = *src.get(pos)?;
                pos += 1;
                lit_len = lit_len.checked_add(extra as usize)?;
                if extra != 255 {
                    break;
                }
            }
        }
        let lits = src.get(pos..pos + lit_len)?;
        pos += lit_len;
        if out.len() + lits.len() > MAX_DECOMPRESSED_BYTES {
            return None;
        }
        out.extend_from_slice(lits);
        if pos >= src.len() {
            break;
        }
        let offset = u16::from_le_bytes([*src.get(pos)?, *src.get(pos + 1)?]) as usize;
        pos += 2;
        if offset == 0 || offset > out.len() {
            return None;
        }
        let mut match_len = ((token & 0x0F) as usize) + 4;
        if (token & 0x0F) == 15 {
            loop {
                let extra = *src.get(pos)?;
                pos += 1;
                match_len = match_len.checked_add(extra as usize)?;
                if extra != 255 {
                    break;
                }
            }
        }
        if out.len() + match_len > MAX_DECOMPRESSED_BYTES {
            return None;
        }
        for _ in 0..match_len {
            let b = out[out.len() - offset];
            out.push(b);
        }
    }
    Some(())
}

fn decode_structured_payload(bytes: &[u8]) -> Option<(&'static str, Value)> {
    // 1. Binary Plist / NSKeyedArchiver
    if bytes.starts_with(b"bplist0") || bytes.starts_with(b"bplist1") {
        if let Ok(val) = plist::Value::from_reader(Cursor::new(bytes)) {
            if let Some(unarchived) = try_unarchive_nskeyed(&val) {
                return Some(("nskeyedarchiver", unarchived));
            }
            return Some(("bplist", plist_to_json(&val, 0)));
        }
        if let Ok(val) = crate::core::preview::plist_tolerant::parse_bplist_to_json(bytes) {
            return Some(("bplist", val));
        }
    }

    // 2. JSON object / array
    let trimmed = bytes
        .iter()
        .copied()
        .skip_while(|b| b.is_ascii_whitespace())
        .collect::<Vec<u8>>();
    if trimmed.starts_with(b"{") || trimmed.starts_with(b"[") {
        if let Ok(text) = std::str::from_utf8(bytes) {
            if let Ok(val) = serde_json::from_str::<Value>(text) {
                return Some(("json", val));
            }
        }
    }

    // 3. XML plist
    if (trimmed.starts_with(b"<?xml") || trimmed.starts_with(b"<plist"))
        && bytes.windows(6).any(|w| w == b"<plist")
    {
        if let Ok(val) = plist::Value::from_reader_xml(Cursor::new(bytes)) {
            if let Some(unarchived) = try_unarchive_nskeyed(&val) {
                return Some(("nskeyedarchiver", unarchived));
            }
            return Some(("xml-plist", plist_to_json(&val, 0)));
        }
    }

    // 4. Schema-less Protobuf wire message
    if let Some(pb) = decode_protobuf_strict(bytes, 0) {
        return Some(("protobuf", pb));
    }

    // 5. MessagePack map/array consuming the entire payload cleanly
    if let Some(mp) = decode_msgpack_strict(bytes) {
        return Some(("msgpack", mp));
    }

    None
}

fn try_unarchive_nskeyed(root: &plist::Value) -> Option<Value> {
    let dict = root.as_dictionary()?;
    let archiver = dict.get("$archiver")?.as_string()?;
    if archiver != "NSKeyedArchiver" {
        return None;
    }
    let objects = dict.get("$objects")?.as_array()?;
    let top = dict.get("$top")?.as_dictionary()?;
    let mut out = Map::new();
    let mut visited = HashSet::new();
    for (k, v) in top {
        out.insert(
            k.clone(),
            resolve_nskeyed_value(v, objects, &mut visited, 0),
        );
    }
    Some(Value::Object(out))
}

fn resolve_nskeyed_value(
    val: &plist::Value,
    objects: &[plist::Value],
    visited: &mut HashSet<u64>,
    depth: usize,
) -> Value {
    if depth >= MAX_PLIST_DEPTH {
        return Value::String("…".to_string());
    }
    if let Some(uid) = val.as_uid() {
        let idx = uid.get();
        if idx == 0 {
            return Value::Null;
        }
        let Some(target) = objects.get(idx as usize) else {
            return Value::String(format!("UID({idx})"));
        };
        if !visited.insert(idx) {
            return Value::String(format!("Cycle(UID({idx}))"));
        }
        let resolved = resolve_nskeyed_object(target, objects, visited, depth + 1);
        visited.remove(&idx);
        return resolved;
    }
    plist_to_json(val, depth)
}

fn resolve_nskeyed_object(
    target: &plist::Value,
    objects: &[plist::Value],
    visited: &mut HashSet<u64>,
    depth: usize,
) -> Value {
    let Some(dict) = target.as_dictionary() else {
        return plist_to_json(target, depth);
    };
    let mut map = Map::new();
    // Resolve NSDictionary (`NS.keys` + `NS.objects`) or NSArray (`NS.objects`) directly.
    if let (Some(keys), Some(vals)) = (
        dict.get("NS.keys").and_then(plist::Value::as_array),
        dict.get("NS.objects").and_then(plist::Value::as_array),
    ) {
        for (k, v) in keys.iter().zip(vals.iter()) {
            let key_json = resolve_nskeyed_value(k, objects, visited, depth + 1);
            let key_str = key_json
                .as_str()
                .map(ToString::to_string)
                .unwrap_or_else(|| key_json.to_string());
            map.insert(
                key_str,
                resolve_nskeyed_value(v, objects, visited, depth + 1),
            );
        }
        return Value::Object(map);
    }
    if let Some(items) = dict.get("NS.objects").and_then(plist::Value::as_array) {
        return Value::Array(
            items
                .iter()
                .map(|v| resolve_nskeyed_value(v, objects, visited, depth + 1))
                .collect(),
        );
    }
    for (k, v) in dict {
        if k == "$class" {
            if let Some(uid) = v.as_uid() {
                if let Some(class_dict) = objects
                    .get(uid.get() as usize)
                    .and_then(plist::Value::as_dictionary)
                {
                    if let Some(classname) = class_dict
                        .get("$classname")
                        .and_then(plist::Value::as_string)
                    {
                        map.insert("$class".to_string(), Value::String(classname.to_string()));
                        continue;
                    }
                }
            }
        }
        map.insert(
            k.clone(),
            resolve_nskeyed_value(v, objects, visited, depth + 1),
        );
    }
    Value::Object(map)
}

fn plist_to_json(value: &plist::Value, depth: usize) -> Value {
    if depth >= MAX_PLIST_DEPTH {
        return Value::String("…".to_string());
    }
    match value {
        plist::Value::Array(items) => {
            Value::Array(items.iter().map(|i| plist_to_json(i, depth + 1)).collect())
        }
        plist::Value::Dictionary(dict) => {
            let mut map = Map::new();
            for (k, v) in dict {
                map.insert(k.clone(), plist_to_json(v, depth + 1));
            }
            Value::Object(map)
        }
        plist::Value::Boolean(b) => Value::Bool(*b),
        plist::Value::Integer(n) => n
            .as_signed()
            .map(|i| Value::Number(Number::from(i)))
            .or_else(|| n.as_unsigned().map(|u| Value::Number(Number::from(u))))
            .unwrap_or(Value::Null),
        plist::Value::Real(r) => Number::from_f64(*r)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        plist::Value::String(s) => Value::String(s.clone()),
        plist::Value::Date(d) => Value::String(d.to_xml_format()),
        plist::Value::Data(b) => {
            if let Some((fmt, inner)) = decode_structured_payload(b) {
                let mut m = Map::new();
                m.insert(
                    "_embeddedFormat".to_string(),
                    Value::String(fmt.to_string()),
                );
                m.insert("value".to_string(), inner);
                Value::Object(m)
            } else {
                Value::String(format!("0x{}", hex::encode(&b[..b.len().min(64)])))
            }
        }
        plist::Value::Uid(u) => Value::String(format!("UID({})", u.get())),
        _ => Value::Null,
    }
}

fn read_pb_varint(bytes: &[u8], pos: &mut usize) -> Option<u64> {
    let mut out = 0u64;
    let mut shift = 0u32;
    for _ in 0..10 {
        let b = *bytes.get(*pos)?;
        *pos += 1;
        out |= u64::from(b & 0x7F) << shift;
        if (b & 0x80) == 0 {
            return Some(out);
        }
        shift += 7;
    }
    None
}

fn is_plausible_text(bytes: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(bytes) else {
        return false;
    };
    !s.is_empty()
        && s.chars()
            .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
}

fn decode_protobuf_strict(bytes: &[u8], depth: usize) -> Option<Value> {
    if bytes.is_empty() || depth >= MAX_PROTO_DEPTH {
        return None;
    }
    let mut pos = 0usize;
    let mut fields = Map::new();
    let mut field_count = 0usize;
    let mut meaningful_evidence = false;

    while pos < bytes.len() {
        let tag = read_pb_varint(bytes, &mut pos)?;
        let field_no = tag >> 3;
        let wire_type = (tag & 0x07) as u8;
        if field_no == 0 || field_no > 0x1FFF_FFFF {
            return None;
        }
        let val = match wire_type {
            0 => {
                let v = read_pb_varint(bytes, &mut pos)?;
                Number::from(v).into()
            }
            1 => {
                let slice: [u8; 8] = bytes.get(pos..pos + 8)?.try_into().ok()?;
                pos += 8;
                let u = u64::from_le_bytes(slice);
                Value::Number(Number::from(u))
            }
            2 => {
                let len = read_pb_varint(bytes, &mut pos)? as usize;
                let end = pos.checked_add(len)?;
                let sub = bytes.get(pos..end)?;
                pos = end;
                if sub.is_empty() {
                    Value::String(String::new())
                } else if is_plausible_text(sub) {
                    if sub.len() >= 3 {
                        meaningful_evidence = true;
                    }
                    Value::String(String::from_utf8_lossy(sub).into_owned())
                } else if let Some((fmt, inner)) = decode_structured_payload_nested(sub, depth + 1)
                {
                    meaningful_evidence = true;
                    let mut m = Map::new();
                    m.insert("_format".to_string(), Value::String(fmt.to_string()));
                    m.insert("data".to_string(), inner);
                    Value::Object(m)
                } else {
                    Value::String(format!("0x{}", hex::encode(&sub[..sub.len().min(64)])))
                }
            }
            5 => {
                let slice: [u8; 4] = bytes.get(pos..pos + 4)?.try_into().ok()?;
                pos += 4;
                let u = u32::from_le_bytes(slice);
                Value::Number(Number::from(u))
            }
            _ => return None,
        };
        field_count += 1;
        let key = format!("f{field_no}");
        match fields.get_mut(&key) {
            Some(Value::Array(arr)) => arr.push(val),
            Some(existing) => {
                let prev = existing.take();
                *existing = Value::Array(vec![prev, val]);
            }
            None => {
                fields.insert(key, val);
            }
        }
    }

    if field_count == 0 || (field_count == 1 && !meaningful_evidence) {
        return None;
    }
    Some(Value::Object(fields))
}

fn decode_structured_payload_nested(bytes: &[u8], depth: usize) -> Option<(&'static str, Value)> {
    if bytes.starts_with(b"bplist0") || bytes.starts_with(b"bplist1") {
        if let Ok(val) = plist::Value::from_reader(Cursor::new(bytes)) {
            if let Some(unarchived) = try_unarchive_nskeyed(&val) {
                return Some(("nskeyedarchiver", unarchived));
            }
            return Some(("bplist", plist_to_json(&val, 0)));
        }
        if let Ok(val) = crate::core::preview::plist_tolerant::parse_bplist_to_json(bytes) {
            return Some(("bplist", val));
        }
    }
    if bytes.starts_with(b"{") || bytes.starts_with(b"[") {
        if let Ok(text) = std::str::from_utf8(bytes) {
            if let Ok(val) = serde_json::from_str::<Value>(text) {
                return Some(("json", val));
            }
        }
    }
    decode_protobuf_strict(bytes, depth).map(|v| ("protobuf", v))
}

fn decode_msgpack_strict(bytes: &[u8]) -> Option<Value> {
    let first = *bytes.first()?;
    let is_container = matches!(first, 0x80..=0x9F | 0xDC..=0xDF);
    if !is_container {
        return None;
    }
    let mut cursor = Cursor::new(bytes);
    let val = rmpv::decode::read_value(&mut cursor).ok()?;
    if cursor.position() != bytes.len() as u64 {
        return None;
    }
    let json = msgpack_to_json(&val, 0)?;
    Some(json)
}

fn msgpack_to_json(val: &rmpv::Value, depth: usize) -> Option<Value> {
    if depth >= MAX_PLIST_DEPTH {
        return Some(Value::String("…".to_string()));
    }
    Some(match val {
        rmpv::Value::Nil => Value::Null,
        rmpv::Value::Boolean(b) => Value::Bool(*b),
        rmpv::Value::Integer(i) => i
            .as_i64()
            .map(|v| Value::Number(Number::from(v)))
            .or_else(|| i.as_u64().map(|v| Value::Number(Number::from(v))))
            .unwrap_or(Value::Null),
        rmpv::Value::F32(f) => Number::from_f64(f64::from(*f))
            .map(Value::Number)
            .unwrap_or(Value::Null),
        rmpv::Value::F64(f) => Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        rmpv::Value::String(s) => Value::String(s.as_str()?.to_string()),
        rmpv::Value::Binary(b) => {
            Value::String(format!("0x{}", hex::encode(&b[..b.len().min(64)])))
        }
        rmpv::Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len().min(128));
            for item in arr.iter().take(128) {
                out.push(msgpack_to_json(item, depth + 1)?);
            }
            Value::Array(out)
        }
        rmpv::Value::Map(entries) => {
            let mut map = Map::new();
            for (k, v) in entries.iter().take(128) {
                let ks = match k {
                    rmpv::Value::String(s) => s.as_str()?.to_string(),
                    rmpv::Value::Integer(i) => i.to_string(),
                    _ => return None,
                };
                map.insert(ks, msgpack_to_json(v, depth + 1)?);
            }
            Value::Object(map)
        }
        rmpv::Value::Ext(t, d) => Value::String(format!("ext({t}, {}B)", d.len())),
    })
}

fn truncate_json_value(value: Value, depth: usize) -> Value {
    if depth >= MAX_PLIST_DEPTH {
        return Value::String("…".to_string());
    }
    match value {
        Value::String(s) => {
            if s.chars().count() > 1024 {
                let mut out: String = s.chars().take(1024).collect();
                out.push('…');
                Value::String(out)
            } else {
                Value::String(s)
            }
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .take(64)
                .map(|v| truncate_json_value(v, depth + 1))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .take(128)
                .map(|(k, v)| (k, truncate_json_value(v, depth + 1)))
                .collect(),
        ),
        other => other,
    }
}

fn compact_summary(format: &str, detail: &Value) -> String {
    let raw = serde_json::to_string(detail).unwrap_or_default();
    let trimmed: String = raw.chars().take(MAX_SUMMARY_CHARS).collect();
    if raw.chars().count() > MAX_SUMMARY_CHARS {
        format!("[{format}] {trimmed}…")
    } else {
        format!("[{format}] {trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn decodes_nested_protobuf_and_zlib_json() {
        // Construct nested protobuf: f1 = 42, f2 = "hello_pb"
        let inner = vec![
            0x08, 42, 0x12, 8, b'h', b'e', b'l', b'l', b'o', b'_', b'p', b'b',
        ];
        let decoded = decode_blob(&inner).expect("protobuf decoded");
        assert_eq!(decoded.format, "protobuf");
        assert_eq!(decoded.detail["f1"], serde_json::json!(42));
        assert_eq!(decoded.detail["f2"], serde_json::json!("hello_pb"));

        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(br#"{"msg":"compressed-ok","n":7}"#)
            .unwrap();
        let compressed = encoder.finish().unwrap();
        let decoded_z = decode_blob(&compressed).expect("zlib+json decoded");
        assert_eq!(decoded_z.format, "zlib+json");
        assert_eq!(decoded_z.detail["msg"], serde_json::json!("compressed-ok"));
    }

    #[test]
    fn unarchives_nskeyedarchiver_bplist() {
        let mut root = plist::Dictionary::new();
        root.insert(
            "$archiver".into(),
            plist::Value::String("NSKeyedArchiver".into()),
        );
        root.insert("$version".into(), plist::Value::Integer(100_000.into()));
        let mut top = plist::Dictionary::new();
        top.insert("root".into(), plist::Value::Uid(plist::Uid::new(1)));
        root.insert("$top".into(), plist::Value::Dictionary(top));
        let mut obj = plist::Dictionary::new();
        obj.insert("sender".into(), plist::Value::Uid(plist::Uid::new(2)));
        let objs = vec![
            plist::Value::String("$null".into()),
            plist::Value::Dictionary(obj),
            plist::Value::String("alice@forensics".into()),
        ];
        root.insert("$objects".into(), plist::Value::Array(objs));
        let mut buf = Vec::new();
        plist::to_writer_binary(&mut buf, &plist::Value::Dictionary(root)).unwrap();

        let decoded = decode_blob(&buf).expect("nskeyedarchiver decoded");
        assert_eq!(decoded.format, "nskeyedarchiver");
        assert_eq!(
            decoded.detail["root"]["sender"],
            serde_json::json!("alice@forensics")
        );
    }

    #[test]
    fn decodes_lz4_frame_payload() {
        // Minimal valid LZ4 frame: magic + FLG(0x60=version 1, B.Indep) + BD(0x40) + HC(0x82) +
        // uncompressed block (high bit set, len=13 `{"ok":12345}`) + endmark(0)
        let payload = br#"{"ok":123456}"#;
        let mut frame = vec![0x04, 0x22, 0x4D, 0x18, 0x60, 0x40, 0x82];
        let blk_hdr = (payload.len() as u32) | 0x8000_0000;
        frame.extend_from_slice(&blk_hdr.to_le_bytes());
        frame.extend_from_slice(payload);
        frame.extend_from_slice(&0u32.to_le_bytes());

        let decoded = decode_blob(&frame).expect("lz4+json decoded");
        assert_eq!(decoded.format, "lz4+json");
        assert_eq!(decoded.detail["ok"], serde_json::json!(123456));
    }
}
