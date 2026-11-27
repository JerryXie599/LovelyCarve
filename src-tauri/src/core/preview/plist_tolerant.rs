//! Tolerant binary plist (`bplist00`) decoder that converts objects directly to JSON.
//!
//! Used as a fallback when the upstream `plist` crate fails on `0x00` Null objects
//! (e.g. `NullObjectUnimplemented`) present in some iOS/macOS preference files.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, SecondsFormat};
use serde_json::{Map, Number, Value};

const HEADER_LEN: usize = 8;
const TRAILER_LEN: usize = 32;
const APPLE_EPOCH_UNIX_SECS: f64 = 978_307_200.0;
const MAX_DEPTH: usize = 128;
const MAX_DECODED_NODES: usize = 500_000;

struct BplistReader<'a> {
    bytes: &'a [u8],
    offset_table_offset: usize,
    offset_int_size: usize,
    object_ref_size: usize,
    num_objects: u64,
    trailer_offset: usize,
}

impl<'a> BplistReader<'a> {
    fn parse(bytes: &'a [u8]) -> Result<(Self, u64)> {
        if bytes.len() < HEADER_LEN + TRAILER_LEN {
            bail!("bplist buffer too short ({} bytes)", bytes.len());
        }
        if !bytes.starts_with(b"bplist0") {
            bail!("not a bplist0x header");
        }
        let trailer_offset = bytes.len() - TRAILER_LEN;
        let trailer = &bytes[trailer_offset..];
        let offset_int_size = usize::from(trailer[6]);
        let object_ref_size = usize::from(trailer[7]);
        if !matches!(offset_int_size, 1 | 2 | 4 | 8) {
            bail!("invalid bplist offset_int_size: {offset_int_size}");
        }
        if !matches!(object_ref_size, 1 | 2 | 4 | 8) {
            bail!("invalid bplist object_ref_size: {object_ref_size}");
        }

        let num_objects = u64::from_be_bytes(trailer[8..16].try_into()?);
        let top_object = u64::from_be_bytes(trailer[16..24].try_into()?);
        let offset_table_u64 = u64::from_be_bytes(trailer[24..32].try_into()?);

        if num_objects == 0 {
            bail!("bplist has zero objects");
        }
        if top_object >= num_objects {
            bail!("bplist top_object index {top_object} >= num_objects {num_objects}");
        }
        let offset_table_offset =
            usize::try_from(offset_table_u64).context("bplist offset_table_offset overflow")?;
        let table_bytes = usize::try_from(num_objects)
            .ok()
            .and_then(|n| n.checked_mul(offset_int_size))
            .context("bplist offset table size overflow")?;
        let table_end = offset_table_offset
            .checked_add(table_bytes)
            .context("bplist offset table end overflow")?;
        if offset_table_offset < HEADER_LEN || table_end > trailer_offset {
            bail!(
                "bplist offset table [{offset_table_offset}..{table_end}) out of range [8..{trailer_offset})"
            );
        }

        Ok((
            Self {
                bytes,
                offset_table_offset,
                offset_int_size,
                object_ref_size,
                num_objects,
                trailer_offset,
            },
            top_object,
        ))
    }

    fn read_be_u64(slice: &[u8]) -> Result<u64> {
        if slice.is_empty() || slice.len() > 8 {
            bail!("invalid big-endian integer width {}", slice.len());
        }
        let mut buf = [0u8; 8];
        buf[8 - slice.len()..].copy_from_slice(slice);
        Ok(u64::from_be_bytes(buf))
    }

    fn object_offset(&self, obj_idx: u64) -> Result<usize> {
        if obj_idx >= self.num_objects {
            bail!(
                "bplist object reference {obj_idx} >= num_objects {}",
                self.num_objects
            );
        }
        let rel = usize::try_from(obj_idx)
            .ok()
            .and_then(|i| i.checked_mul(self.offset_int_size))
            .and_then(|off| self.offset_table_offset.checked_add(off))
            .context("bplist object table entry overflow")?;
        let end = rel
            .checked_add(self.offset_int_size)
            .context("bplist object table entry overflow")?;
        if end > self.trailer_offset {
            bail!("bplist offset table entry out of bounds");
        }
        let raw = Self::read_be_u64(&self.bytes[rel..end])?;
        let offset = usize::try_from(raw).context("bplist object offset overflow")?;
        if offset < HEADER_LEN || offset >= self.trailer_offset {
            bail!("bplist object offset {offset} out of bounds");
        }
        Ok(offset)
    }

    fn read_length(&self, pos: &mut usize, low: u8) -> Result<usize> {
        if low < 0x0f {
            return Ok(usize::from(low));
        }
        if *pos >= self.trailer_offset {
            bail!("truncated bplist extended length marker");
        }
        let marker = self.bytes[*pos];
        *pos += 1;
        if (marker >> 4) != 0x1 {
            bail!("invalid bplist extended length marker {marker:#04x}");
        }
        let exp = marker & 0x0f;
        if exp > 3 {
            bail!("unsupported bplist extended length width 2^{exp}");
        }
        let nbytes = 1usize << exp;
        let end = pos
            .checked_add(nbytes)
            .context("bplist extended length overflow")?;
        if end > self.trailer_offset {
            bail!("truncated bplist extended length value");
        }
        let val = Self::read_be_u64(&self.bytes[*pos..end])?;
        *pos = end;
        usize::try_from(val).context("bplist extended length exceeds usize")
    }

    fn slice_at(&self, pos: &mut usize, len: usize) -> Result<&'a [u8]> {
        let end = pos.checked_add(len).context("bplist slice overflow")?;
        if end > self.trailer_offset {
            bail!("truncated bplist payload at offset {pos}");
        }
        let s = &self.bytes[*pos..end];
        *pos = end;
        Ok(s)
    }

    fn decode(
        &self,
        obj_idx: u64,
        depth: usize,
        visiting: &mut Vec<u64>,
        node_count: &mut usize,
    ) -> Result<Value> {
        if depth >= MAX_DEPTH {
            bail!("bplist nesting depth exceeds {MAX_DEPTH}");
        }
        *node_count += 1;
        if *node_count > MAX_DECODED_NODES {
            bail!("bplist decoded node count exceeds {MAX_DECODED_NODES}");
        }
        if visiting.contains(&obj_idx) {
            bail!("bplist contains cyclic reference at object #{obj_idx}");
        }
        visiting.push(obj_idx);
        let res = self.decode_inner(obj_idx, depth, visiting, node_count);
        visiting.pop();
        res
    }

    fn decode_inner(
        &self,
        obj_idx: u64,
        depth: usize,
        visiting: &mut Vec<u64>,
        node_count: &mut usize,
    ) -> Result<Value> {
        let mut pos = self.object_offset(obj_idx)?;
        let marker = self.bytes[pos];
        pos += 1;
        let high = marker >> 4;
        let low = marker & 0x0f;

        match high {
            0x0 => match low {
                0x0 | 0xf => Ok(Value::Null),
                0x8 => Ok(Value::Bool(false)),
                0x9 => Ok(Value::Bool(true)),
                _ => bail!("unsupported bplist singleton marker {marker:#04x}"),
            },
            0x1 => {
                if low > 4 {
                    bail!("unsupported bplist int byte exponent {low}");
                }
                let nbytes = 1usize << low;
                let s = self.slice_at(&mut pos, nbytes)?;
                match nbytes {
                    1 | 2 | 4 => Ok(Value::Number(Number::from(Self::read_be_u64(s)?))),
                    8 => {
                        let arr: [u8; 8] = s.try_into()?;
                        Ok(Value::Number(Number::from(i64::from_be_bytes(arr))))
                    }
                    16 => {
                        let arr: [u8; 16] = s.try_into()?;
                        let v = i128::from_be_bytes(arr);
                        if let Ok(signed) = i64::try_from(v) {
                            Ok(Value::Number(Number::from(signed)))
                        } else if let Ok(unsigned) = u64::try_from(v) {
                            Ok(Value::Number(Number::from(unsigned)))
                        } else {
                            Ok(Value::String(v.to_string()))
                        }
                    }
                    _ => unreachable!(),
                }
            }
            0x2 => match low {
                2 => {
                    let arr: [u8; 4] = self.slice_at(&mut pos, 4)?.try_into()?;
                    let val = f64::from(f32::from_be_bytes(arr));
                    Ok(Number::from_f64(val)
                        .map(Value::Number)
                        .unwrap_or(Value::Null))
                }
                3 => {
                    let arr: [u8; 8] = self.slice_at(&mut pos, 8)?.try_into()?;
                    let val = f64::from_be_bytes(arr);
                    Ok(Number::from_f64(val)
                        .map(Value::Number)
                        .unwrap_or(Value::Null))
                }
                _ => bail!("unsupported bplist real byte exponent {low}"),
            },
            0x3 => {
                if low != 3 {
                    bail!("unsupported bplist date size exponent {low}");
                }
                let arr: [u8; 8] = self.slice_at(&mut pos, 8)?.try_into()?;
                Ok(format_apple_date(f64::from_be_bytes(arr)))
            }
            0x4 => {
                let len = self.read_length(&mut pos, low)?;
                let data = self.slice_at(&mut pos, len)?;
                Ok(Value::String(hex::encode(data)))
            }
            0x5 => {
                let len = self.read_length(&mut pos, low)?;
                let raw = self.slice_at(&mut pos, len)?;
                Ok(Value::String(String::from_utf8_lossy(raw).into_owned()))
            }
            0x6 => {
                let code_units = self.read_length(&mut pos, low)?;
                let byte_len = code_units
                    .checked_mul(2)
                    .context("bplist UTF-16 byte length overflow")?;
                let raw = self.slice_at(&mut pos, byte_len)?;
                let units: Vec<u16> = raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&pair| u16::from_be_bytes(pair))
                    .collect();
                Ok(Value::String(String::from_utf16_lossy(&units)))
            }
            0x8 => {
                let nbytes = usize::from(low) + 1;
                if nbytes > 8 {
                    bail!("unsupported bplist UID width {nbytes}");
                }
                let uid = Self::read_be_u64(self.slice_at(&mut pos, nbytes)?)?;
                Ok(Value::String(format!("UID({uid})")))
            }
            0xa | 0xc => {
                let count = self.read_length(&mut pos, low)?;
                let refs_bytes = count
                    .checked_mul(self.object_ref_size)
                    .context("bplist array refs size overflow")?;
                let refs = self.slice_at(&mut pos, refs_bytes)?;
                let mut out = Vec::with_capacity(count.min(4096));
                for chunk in refs.chunks_exact(self.object_ref_size) {
                    let child_idx = Self::read_be_u64(chunk)?;
                    out.push(self.decode(child_idx, depth + 1, visiting, node_count)?);
                }
                Ok(Value::Array(out))
            }
            0xd => {
                let count = self.read_length(&mut pos, low)?;
                let pairs_bytes = count
                    .checked_mul(2)
                    .and_then(|n| n.checked_mul(self.object_ref_size))
                    .context("bplist dict refs size overflow")?;
                let refs = self.slice_at(&mut pos, pairs_bytes)?;
                let half = count * self.object_ref_size;
                let (key_refs, val_refs) = refs.split_at(half);
                let mut map = Map::with_capacity(count.min(4096));
                for (k_chunk, v_chunk) in key_refs
                    .chunks_exact(self.object_ref_size)
                    .zip(val_refs.chunks_exact(self.object_ref_size))
                {
                    let k_idx = Self::read_be_u64(k_chunk)?;
                    let v_idx = Self::read_be_u64(v_chunk)?;
                    let key = value_to_key(self.decode(k_idx, depth + 1, visiting, node_count)?);
                    let val = self.decode(v_idx, depth + 1, visiting, node_count)?;
                    map.insert(key, val);
                }
                Ok(Value::Object(map))
            }
            _ => bail!("unsupported bplist object marker {marker:#04x}"),
        }
    }
}

fn value_to_key(val: Value) -> String {
    match val {
        Value::String(s) => s,
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn format_apple_date(apple_secs: f64) -> Value {
    if !apple_secs.is_finite() {
        return Value::Null;
    }
    let unix = apple_secs + APPLE_EPOCH_UNIX_SECS;
    if unix < (i64::MIN as f64) || unix > (i64::MAX as f64) {
        return Value::Null;
    }
    let secs = unix.trunc() as i64;
    let nanos_f = ((unix - (secs as f64)) * 1_000_000_000.0).round();
    let (adj_secs, nanos) = if nanos_f < 0.0 {
        (secs.saturating_sub(1), (1_000_000_000.0 + nanos_f) as u32)
    } else if nanos_f >= 1_000_000_000.0 {
        (secs.saturating_add(1), 0)
    } else {
        (secs, nanos_f as u32)
    };
    DateTime::from_timestamp(adj_secs, nanos)
        .map(|dt| Value::String(dt.to_rfc3339_opts(SecondsFormat::Secs, true)))
        .unwrap_or(Value::Null)
}

/// Parses an Apple `bplist00` buffer into JSON, supporting `0x00` Null objects.
pub fn parse_bplist_to_json(bytes: &[u8]) -> Result<Value> {
    let (reader, top_object) = BplistReader::parse(bytes)?;
    let mut visiting = Vec::new();
    let mut node_count = 0;
    reader.decode(top_object, 0, &mut visiting, &mut node_count)
}

#[cfg(test)]
mod tests {
    use super::parse_bplist_to_json;
    use serde_json::json;

    fn build_bplist(objects: Vec<Vec<u8>>, top: u64) -> Vec<u8> {
        let mut buf = b"bplist00".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for obj in &objects {
            offsets.push(buf.len() as u16);
            buf.extend_from_slice(obj);
        }
        let offset_table_offset = buf.len() as u64;
        for off in offsets {
            buf.extend_from_slice(&off.to_be_bytes());
        }
        buf.extend_from_slice(&[0u8; 6]);
        buf.push(2); // offset_int_size
        buf.push(1); // object_ref_size
        buf.extend_from_slice(&(objects.len() as u64).to_be_bytes());
        buf.extend_from_slice(&top.to_be_bytes());
        buf.extend_from_slice(&offset_table_offset.to_be_bytes());
        buf
    }

    #[test]
    fn parses_null_utf16_date_data_uid_and_extended_length() {
        let long_ascii = "abcdefghij123456"; // 16 bytes -> ext length 0x5f 0x10 16
        let mut long_obj = vec![0x5f, 0x10, long_ascii.len() as u8];
        long_obj.extend_from_slice(long_ascii.as_bytes());

        let utf16_units: Vec<u16> = "鸿蒙".encode_utf16().collect();
        let mut utf16_obj = vec![0x62];
        for u in utf16_units {
            utf16_obj.extend_from_slice(&u.to_be_bytes());
        }

        let mut date_obj = vec![0x33];
        date_obj.extend_from_slice(&0.0f64.to_be_bytes()); // 2001-01-01T00:00:00Z

        let mut dict = vec![0xd7];
        dict.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7]); // 7 keys
        dict.extend_from_slice(&[8, 9, 10, 11, 12, 13, 14]); // 7 values

        let mut int64 = vec![0x13];
        int64.extend_from_slice(&(-42i64).to_be_bytes());

        let mut real = vec![0x23];
        real.extend_from_slice(&1.5f64.to_be_bytes());

        let objects = vec![
            dict,                    // 0: root dict
            b"\x57expires".to_vec(), // 1
            b"\x54text".to_vec(),    // 2
            b"\x54date".to_vec(),    // 3
            b"\x53raw".to_vec(),     // 4
            b"\x53uid".to_vec(),     // 5
            b"\x54long".to_vec(),    // 6
            b"\x54list".to_vec(),    // 7
            vec![0x00],              // 8: Null!
            utf16_obj,               // 9
            date_obj,                // 10
            vec![0x42, 0xca, 0xfe],  // 11: Data
            vec![0x80, 7],           // 12: UID(7)
            long_obj,                // 13
            vec![0xa3, 8, 15, 16],   // 14: [null, -42, 1.5]
            int64,                   // 15
            real,                    // 16
        ];

        let raw = build_bplist(objects, 0);
        // Upstream `plist` fails on Null (0x00), whereas tolerant succeeds.
        assert!(plist::Value::from_reader(std::io::Cursor::new(&raw)).is_err());
        let parsed = parse_bplist_to_json(&raw).unwrap();
        assert_eq!(
            parsed,
            json!({
                "expires": null,
                "text": "鸿蒙",
                "date": "2001-01-01T00:00:00Z",
                "raw": "cafe",
                "uid": "UID(7)",
                "long": long_ascii,
                "list": [null, -42, 1.5]
            })
        );
    }

    #[test]
    fn supports_shared_dag_strings_and_rejects_cycles_and_truncation() {
        let mut dict = vec![0xd2, 1, 2, 3, 3]; // keys 1,2 both share value 3
        dict.resize(5, 0);
        let dag = build_bplist(
            vec![
                dict,
                b"\x51a".to_vec(),
                b"\x51b".to_vec(),
                b"\x56shared".to_vec(),
            ],
            0,
        );
        assert_eq!(
            parse_bplist_to_json(&dag).unwrap(),
            json!({ "a": "shared", "b": "shared" })
        );

        let cyclic = build_bplist(vec![vec![0xa1, 0]], 0);
        assert!(parse_bplist_to_json(&cyclic).is_err());
        assert!(parse_bplist_to_json(&cyclic[..cyclic.len() - 4]).is_err());
    }
}
