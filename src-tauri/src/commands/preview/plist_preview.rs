//! plist 结构化预览：把 bplist00 / XML plist 解析为 JSON 树，供前端结构化
//! 查看器（图 / 树视图）渲染。plist 格式里没有 JSON 对应物的类型按以下规则
//! 降级：日期 → RFC3339 字符串，data → hex 字符串，UID → `UID(n)` 字符串。
//!
//! 真实检材的 plist 常见越界日期 / 截断（如 `var/Info.plist` 半途而废），
//! 解析全程 catch_unwind 兜底，失败时把原因交回前端降级为原始视图。

use std::io::Cursor;
use std::panic::{catch_unwind, AssertUnwindSafe};

use serde::Serialize;

use crate::core::common::ids::{ImageId, NodeId};
use crate::core::filesystem::tree::fs_tree;
use crate::state::AppState;
use crate::{app::access::loaded_image_app_error, app::error::AppError, host::State};

/// 与 `fs_read` 相同的单次读取上限；超过即标记 truncated 并解析前缀。
const MAX_PLIST_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Serialize)]
pub struct PlistPreview {
    /// 解析格式：`bplist` | `xml`。
    pub format: &'static str,
    /// 根值序列化成的 JSON 文本；解析失败为 None。
    pub json: Option<String>,
    /// 文件超过读取上限，只解析了前缀。
    pub truncated: bool,
    pub error: Option<String>,
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_plist_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
) -> Result<PlistPreview, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let loaded = tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, u64), AppError> {
        let mut session = fs_tree::FsSession::new(&image);
        let stat = session.file_stat(&node_id).map_err(AppError::from)?;
        let data = session
            .read_file_limited(&node_id, MAX_PLIST_BYTES as usize)
            .map_err(AppError::from)?;
        Ok((data, stat.size))
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_plist_preview read", err))??;
    let (bytes, size) = loaded;
    let truncated = size > MAX_PLIST_BYTES;

    let parsed = tokio::task::spawn_blocking(move || parse_lenient(&bytes))
        .await
        .map_err(|err| AppError::task_panicked("fs_plist_preview parse", err))?;
    Ok(match parsed {
        Ok((format, json)) => PlistPreview {
            format,
            json: Some(json),
            truncated,
            error: None,
        },
        Err(error) => PlistPreview {
            format: "bplist",
            json: None,
            truncated,
            error: Some(error),
        },
    })
}

/// 宽限解析：bplist00 / bplist1x 魔数走二进制解析器，否则当 XML；解析
/// panic 视为损坏文件。
fn parse_bplist_fallback(bytes: &[u8]) -> Option<Result<(&'static str, String), String>> {
    let value = crate::core::preview::plist_tolerant::parse_bplist_to_json(bytes).ok()?;
    Some(
        serde_json::to_string(&value)
            .map(|json| ("bplist", json))
            .map_err(|err| format!("JSON 序列化失败：{err}")),
    )
}

fn parse_lenient(bytes: &[u8]) -> Result<(&'static str, String), String> {
    if bytes.is_empty() {
        return Err("空文件".to_string());
    }
    let binary = bytes.starts_with(b"bplist00") || bytes.starts_with(b"bplist1");
    let result = catch_unwind(AssertUnwindSafe(|| {
        if binary {
            plist::Value::from_reader(Cursor::new(bytes))
                .map(|value| (value, "bplist" as &'static str))
        } else {
            plist::Value::from_reader_xml(Cursor::new(bytes))
                .map(|value| (value, "xml" as &'static str))
        }
    }));
    match result {
        Ok(Ok((value, format))) => {
            let json = serde_json::to_string(&value_to_json(&value))
                .map_err(|error| format!("JSON 序列化失败：{error}"))?;
            Ok((format, json))
        }
        Ok(Err(error)) => {
            if binary {
                if let Some(res) = parse_bplist_fallback(bytes) {
                    return res;
                }
            }
            Err(error.to_string())
        }
        Err(_) => {
            if binary {
                if let Some(res) = parse_bplist_fallback(bytes) {
                    return res;
                }
            }
            Err("plist 解析过程 panic（文件可能损坏或包含越界值）".to_string())
        }
    }
}

fn value_to_json(value: &plist::Value) -> serde_json::Value {
    match value {
        plist::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(value_to_json).collect())
        }
        plist::Value::Dictionary(dict) => {
            let mut map = serde_json::Map::with_capacity(dict.len());
            for (key, item) in dict.iter() {
                map.insert(key.clone(), value_to_json(item));
            }
            serde_json::Value::Object(map)
        }
        plist::Value::Boolean(flag) => serde_json::Value::Bool(*flag),
        plist::Value::Integer(number) => number_or_string(number),
        plist::Value::Real(number) => serde_json::Number::from_f64(*number)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        plist::Value::String(text) => serde_json::Value::String(text.clone()),
        plist::Value::Date(date) => serde_json::Value::String(date.to_xml_format()),
        plist::Value::Data(bytes) => serde_json::Value::String(hex::encode(bytes)),
        plist::Value::Uid(uid) => serde_json::Value::String(format!("UID({})", uid.get())),
        _ => serde_json::Value::Null,
    }
}

fn number_or_string(number: &plist::Integer) -> serde_json::Value {
    if let Some(signed) = number.as_signed() {
        return serde_json::Value::Number(serde_json::Number::from(signed));
    }
    match number.as_unsigned() {
        Some(unsigned) => serde_json::Value::Number(serde_json::Number::from(unsigned)),
        None => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bplist_and_degrades_data_to_hex() {
        // 用 plist 库自己构造一个二进制 plist 再回读，覆盖 end-to-end。
        let mut dict = plist::Dictionary::new();
        dict.insert(
            "name".to_string(),
            plist::Value::String("paprika".to_string()),
        );
        dict.insert(
            "count".to_string(),
            plist::Value::Integer(plist::Integer::from(42i64)),
        );
        dict.insert("flag".to_string(), plist::Value::Boolean(true));
        dict.insert(
            "blob".to_string(),
            plist::Value::Data(vec![0x89, 0x50, 0x4e, 0x47]),
        );
        let mut buffer = Vec::new();
        plist::to_writer_binary(&mut buffer, &plist::Value::Dictionary(dict)).unwrap();

        let (format, json) = parse_lenient(&buffer).unwrap();
        assert_eq!(format, "bplist");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["name"], "paprika");
        assert_eq!(parsed["count"], 42);
        assert_eq!(parsed["flag"], true);
        assert_eq!(parsed["blob"], "89504e47");
    }

    #[test]
    fn parses_xml_and_reports_corruption() {
        let xml =
            br#"<?xml version="1.0"?><plist><dict><key>k</key><string>v</string></dict></plist>"#;
        let (format, json) = parse_lenient(xml).unwrap();
        assert_eq!(format, "xml");
        assert!(json.contains("\"k\":\"v\""));

        assert!(parse_lenient(b"").is_err());
        assert!(parse_lenient(b"bplist00garbage").is_err());
        assert!(parse_lenient(b"not a plist at all").is_err());
    }
}
