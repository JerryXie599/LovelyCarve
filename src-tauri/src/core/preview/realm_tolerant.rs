//! Realm 容错解析器：面向 11.x / 24.x 新格式文件（vendored
//! `realm-db-reader` 只精读 9.9 布局，Table::build 对新表节点布局会失败）。
//!
//! 逻辑移植自 iOS 备份插件的自研解析器（在同批检材的 SendAnywhere /
//! audio.realm 上验证）：不依赖版本号，按 Realm Core slab 布局（node_header
//! 8 字节：h[4]=标志位 bit7 inner / bit6 hasrefs / bits4-3 wtype /
//! bits2-0 width(log2)，h[5..8)=24-bit 元素数）从 top ref 做 BFS，启发式识别
//! 表节点（child[0]=spec 列名簇、child[2]=列数据簇），列名取 spec 的名字
//! blob（定长 NUL 槽），表名按文件头部 `class_*` 出现顺序对应。
//!
//! 口径：一切列值字符串化（原始枚举/整数/时间戳都不做业务转写），足够
//! 只读预览使用；不追求 vendored reader 的精确类型还原。

const MAX_BYTES: usize = 64 << 20;
const MAX_NODES: usize = 4096;
const MAX_TABLES: usize = 64;
const MAX_VALUES: usize = 2_000;

#[derive(Debug, Clone)]
pub struct TolerantTable {
    pub name: String,
    /// `(列名, 行值)`；各列长度可能不同（空列不产出）。
    pub columns: Vec<(String, Vec<String>)>,
}

pub fn parse_tables(bytes: &[u8]) -> Vec<TolerantTable> {
    let _ = MAX_BYTES;
    let mut seen = std::collections::HashSet::new();
    if bytes.len() < 16 || !bytes[16..20].starts_with(b"T-DB") {
        return Vec::new();
    }
    let top = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize;
    let Some(topn) = hdr(bytes, top) else {
        return Vec::new();
    };
    if !topn.hasrefs {
        return Vec::new();
    }
    let mut tables: Vec<TolerantTable> = Vec::new();
    // FIFO：组内子表按声明顺序发现，表名（class_*）按同一顺序对应。
    let mut queue: std::collections::VecDeque<usize> = read_refs(bytes, &topn)
        .into_iter()
        .filter(|r| *r > 8 && *r < bytes.len())
        .collect();
    let class_names = class_names_from_head(bytes);
    let mut table_idx = 0usize;
    while let Some(off) = queue.pop_front() {
        if seen.len() > MAX_NODES || tables.len() >= MAX_TABLES {
            break;
        }
        if !seen.insert(off) {
            continue;
        }
        let Some(n) = hdr(bytes, off) else { continue };
        if n.inner || !n.hasrefs {
            continue;
        }
        let refs = read_refs(bytes, &n);
        for r in &refs {
            if *r > 8 && *r < bytes.len() {
                queue.push_back(*r);
            }
        }
        // 表节点：child[0]=spec（refs，且 child[1]=名字 blob 叶子，内容为
        // NUL 分隔列名）；child[2]=列簇（簇内 refs[0] 是额外节点，列从
        // refs[1..] 一一对应列名）。
        let Some(spec_ref) = refs.first().copied() else {
            continue;
        };
        let Some(spec) = hdr(bytes, spec_ref) else {
            continue;
        };
        if spec.inner || !spec.hasrefs || spec.size < 3 {
            continue;
        }
        let srefs = read_refs(bytes, &spec);
        let Some(names_ref) = srefs.get(1).copied() else {
            continue;
        };
        let Some(nn) = hdr(bytes, names_ref) else {
            continue;
        };
        if nn.hasrefs || nn.wtype != 1 || nn.width == 0 {
            continue;
        }
        // 名字 blob 为定长槽（槽宽 = (1<<wbits)>>1 字节），槽内 NUL 填充，
        // 部分槽首带 1 字节杂讯；按槽切分后清洗出列名。
        let slot = nn.width;
        if nn.size == 0 {
            continue;
        }
        let raw_end = (nn.data + slot * nn.size).min(bytes.len());
        let mut names: Vec<String> = Vec::new();
        for si in 0..nn.size {
            let sl = &bytes[nn.data + slot * si..raw_end.min(nn.data + slot * (si + 1))];
            let t: String = sl
                .iter()
                .map(|c| *c as char)
                .skip_while(|c| !c.is_ascii_alphanumeric())
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !t.is_empty() {
                names.push(t);
            }
        }
        if names.len() < 3 {
            continue;
        }
        let Some(cl_ref) = refs.get(2).copied() else {
            continue;
        };
        let Some(cl) = hdr(bytes, cl_ref) else {
            continue;
        };
        if cl.inner || !cl.hasrefs {
            continue;
        }
        let crefs = read_refs(bytes, &cl);
        if crefs.len() < names.len() + 1 {
            continue;
        }
        let name = class_names
            .get(table_idx)
            .map(|s| s.trim_start_matches("class_").to_string())
            .unwrap_or_else(|| format!("table{table_idx}"));
        table_idx += 1;
        let mut cols: Vec<(String, Vec<String>)> = Vec::new();
        for (i, cn) in names.iter().enumerate() {
            let Some(cref) = crefs.get(i + 1).copied() else {
                continue;
            };
            let vals = col_strings(bytes, cref)
                .or_else(|| col_timestamps(bytes, cref))
                .filter(|v: &Vec<String>| !v.is_empty())
                .unwrap_or_else(|| col_values(bytes, cref));
            if vals.is_empty() {
                continue;
            }
            cols.push((cn.clone(), vals));
        }
        if !cols.is_empty() {
            tables.push(TolerantTable {
                name,
                columns: cols,
            });
        }
    }
    tables
}

struct Node {
    inner: bool,
    hasrefs: bool,
    wtype: u8,
    /// (1<<wbits)>>1 字节
    width: usize,
    size: usize,
    data: usize,
}

fn hdr(b: &[u8], off: usize) -> Option<Node> {
    if off + 8 > b.len() {
        return None;
    }
    let h = &b[off..off + 8];
    let flags = h[4];
    Some(Node {
        inner: (flags >> 7) & 1 == 1,
        hasrefs: (flags >> 6) & 1 == 1,
        wtype: (flags >> 3) & 3,
        width: (1usize << (flags & 7)) >> 1,
        size: ((h[5] as usize) << 16) + ((h[6] as usize) << 8) + h[7] as usize,
        data: off + 8,
    })
}

fn int_at(b: &[u8], s: usize, eb: usize) -> i64 {
    let mut v: u64 = 0;
    for i in 0..eb.min(8) {
        v |= (b[s + i] as u64) << (8 * i);
    }
    v as i64
}

fn read_refs(b: &[u8], n: &Node) -> Vec<usize> {
    let mut out = Vec::new();
    let eb = match n.wtype {
        0 => n.width / 8,
        _ => n.width,
    };
    if eb == 0 {
        return out;
    }
    for i in 0..n.size.min(64) {
        let s = n.data + eb * i;
        if s + eb > b.len() {
            break;
        }
        out.push(int_at(b, s, eb) as usize);
    }
    out
}

fn leaf_ints(b: &[u8], n: &Node) -> Vec<i64> {
    let eb = match n.wtype {
        1 => n.width,
        0 => n.width / 8,
        _ => 0,
    };
    if eb == 0 || eb > 8 {
        return Vec::new();
    }
    (0..n.size.min(MAX_VALUES))
        .map(|i| int_at(b, n.data + eb * i, eb))
        .collect()
}

/// 字符串列：refs[偏移叶子, blob 叶子(wtype=2), 标志]。
fn col_strings(b: &[u8], col: usize) -> Option<Vec<String>> {
    let n = hdr(b, col)?;
    if n.inner || !n.hasrefs || n.size < 2 || n.size > 4 {
        return None;
    }
    let refs = read_refs(b, &n);
    let offs_node = hdr(b, *refs.first()?)?;
    let mut offs = leaf_ints(b, &offs_node);
    if offs.len() < 2 {
        return None;
    }
    let mut blob: &[u8] = &[];
    for r in refs.iter().skip(1) {
        if let Some(c) = hdr(b, *r) {
            if c.wtype == 2 {
                let end = (c.data + c.size).min(b.len());
                blob = &b[c.data..end];
            }
        }
    }
    if blob.is_empty() {
        return None;
    }
    offs.push(blob.len() as i64);
    offs.sort_unstable();
    let mut out = Vec::new();
    for w in offs.windows(2) {
        if w[0] < 0 || w[1] > blob.len() as i64 || w[1] < w[0] {
            continue;
        }
        let seg = &blob[w[0] as usize..w[1] as usize];
        out.push(
            String::from_utf8_lossy(seg)
                .trim_end_matches('\0')
                .to_string(),
        );
    }
    Some(out)
}

/// 时间戳列：refs[秒叶子, 纳秒叶子]。秒为 Unix 基准，按 UTC+8 呈现
/// （插件口径一致）。
fn col_timestamps(b: &[u8], col: usize) -> Option<Vec<String>> {
    let n = hdr(b, col)?;
    if n.inner || !n.hasrefs || n.size != 2 {
        return None;
    }
    let refs = read_refs(b, &n);
    let secs = leaf_ints(b, &hdr(b, *refs.first()?)?);
    let out: Vec<String> = secs
        .into_iter()
        .filter(|s| *s != i32::MAX as i64 && *s > 1_000_000_000)
        .map(|s| {
            chrono::DateTime::from_timestamp(s + 8 * 3600, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default()
        })
        .collect();
    Some(out)
}

/// 通用列值解码：内联短字符串 / 定宽整数 / 子字节位打包。
fn col_values(b: &[u8], col: usize) -> Vec<String> {
    let Some(n) = hdr(b, col) else {
        return Vec::new();
    };
    if n.inner || n.hasrefs {
        return Vec::new();
    }
    let eb = match n.wtype {
        1 => n.width,
        0 => n.width / 8,
        _ => 0,
    };
    if eb == 0 {
        // 子字节位打包（wtype=0，width 字段即位宽）
        if n.width == 0 {
            return vec!["0".to_string(); n.size.min(MAX_VALUES)];
        }
        let mut out = Vec::new();
        for i in 0..n.size.min(MAX_VALUES) {
            let mut v = 0i64;
            for j in 0..n.width.min(32) {
                let bit = i * n.width + j;
                if let Some(byte) = b.get(n.data + (bit >> 3)) {
                    v |= (((byte >> (bit & 7)) & 1) as i64) << j;
                }
            }
            out.push(v.to_string());
        }
        return out;
    }
    let mut vals = Vec::new();
    let mut all_ascii = eb >= 2;
    for i in 0..n.size.min(MAX_VALUES) {
        let s = n.data + eb * i;
        if s + eb > b.len() {
            break;
        }
        let raw = &b[s..s + eb];
        // 内联字符串槽：首 NUL 之前必须全为可打印字符（NUL 之后的字节为填充/标记）
        let nul = raw.iter().position(|c| *c == 0);
        let head = match nul {
            Some(p) => &raw[..p],
            None => raw,
        };
        let printable = !head.is_empty()
            && head.iter().all(|c| (0x20..0x7f).contains(c))
            && nul
                .map(|p| raw[p + 1..].iter().all(|c| *c == 0 || *c == 1 || *c == 2))
                .unwrap_or(true);
        if all_ascii && printable {
            let t: String = head.iter().map(|c| *c as char).collect();
            vals.push(t);
        } else {
            all_ascii = false;
            vals.push(int_at(b, s, eb.min(8)).to_string());
        }
    }
    if !all_ascii {
        return vals;
    }
    // 8 字节整数列可能是 Double（尺寸/时间等），若全部落合理量级则按 f64 呈现
    if eb == 8 {
        let fvals: Vec<f64> = (0..n.size.min(MAX_VALUES))
            .map(|i| {
                let s = n.data + 8 * i;
                f64::from_le_bytes(b[s..s + 8].try_into().unwrap())
            })
            .collect();
        if fvals.iter().all(|v| v.is_finite() && v.abs() <= 1e10) {
            return fvals
                .into_iter()
                .map(|v| {
                    if v.fract() == 0.0 && v.abs() < 1e15 {
                        format!("{}", v as i64)
                    } else {
                        format!("{v}")
                    }
                })
                .collect();
        }
    }
    vals
}

/// 文件头部 4KB 内的 class_ 表名（按出现顺序）。
fn class_names_from_head(b: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let limit = b.len().min(0x1000);
    let mut i = 0usize;
    while i + 6 < limit {
        match b[i..limit].windows(6).position(|w| w == b"class_") {
            Some(pos) => {
                let start = i + pos;
                let end = b[start..limit]
                    .iter()
                    .position(|c| !c.is_ascii_alphanumeric() && *c != b'_')
                    .map(|p| start + p)
                    .unwrap_or(limit);
                let name = String::from_utf8_lossy(&b[start..end]).to_string();
                if !out.contains(&name) {
                    out.push(name);
                }
                i = end;
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 快照：CHAN_MH 检材 audio.realm（曾被 Realm Studio 原地升级为 24.11）。
    #[test]
    #[ignore = "本机检材快照"]
    fn parses_v24_snapshot() {
        let bytes =
            std::fs::read("E:/software/04-Forensics/LovelyCarve/_audit/audio_realm_snapshot.realm")
                .expect("快照不存在");
        let tables = parse_tables(&bytes);
        assert!(!tables.is_empty(), "容错解析未发现任何表");
        for table in &tables {
            tracing::info!(
                "table {} cols={:?}",
                table.name,
                table
                    .columns
                    .iter()
                    .map(|(n, v)| (n.as_str(), v.len()))
                    .collect::<Vec<_>>()
            );
        }
    }
}
