//! Filesystem native-preview IPC for structured file formats rendered in the
//! original-type view.

use std::sync::Arc;

use crate::host::State;

use crate::core::common::ids::{ImageId, NodeId, NodeRef};
use crate::core::filesystem::evidence_session::EvidenceSession;
use crate::core::filesystem::tree::fs_tree;
use crate::core::filesystem::volumes::ntfs::{parse_ntfs_ads_id, parse_ntfs_id, NtfsSource};
use crate::core::preview::evtx_preview::{self, EvtxDigest, EvtxPage};
use crate::core::preview::mft_preview::{
    self, build_ntfs_record_forensic_view, MftDigest, MftPage, MftRecordDetail, MftSessionCache,
    NtfsRecordForensicView,
};
use crate::core::preview::realm_preview::{self, RealmDigest, RealmRows};
use crate::core::preview::registry_preview::{
    self, RegistryDigest, RegistryKeyDetail, RegistrySearchResult, MAX_REGISTRY_BYTES,
};
use crate::core::preview::sqlite::sqlite_preview::{
    self, SqliteDigest, SqliteKey, SqliteQueryPage, SqliteRows, SqliteSearchResult,
};
use crate::core::preview::sqlite::sqlite_recovery::{self, SqliteRecovery};
use crate::core::preview::vim_swap::{self, VimSwapDigest};
use crate::state::AppState;
use {crate::app::access::loaded_image_app_error, crate::app::error::AppError};

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_sqlite_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
    key: Option<SqliteKey>,
) -> Result<SqliteDigest, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.sqlite_session.clone();
    let key = key.unwrap_or_default();
    tokio::task::spawn_blocking(move || -> Result<SqliteDigest, AppError> {
        let session = sqlite_preview::cached_session(
            &mut cache.blocking_lock(),
            &image,
            &image_id,
            &node_id,
            parent_id.as_deref(),
        )
        .map_err(AppError::from)?;
        sqlite_preview::preview_session(&session, &key).map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_sqlite_preview", err))?
}

/// Recover deleted records from a SQLite database (and its `-wal` or
/// `-journal`). `key` is the key the browser decrypted an encrypted database
/// with; the pages are decrypted again for carving.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_sqlite_deleted_records(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
    key: Option<SqliteKey>,
) -> Result<SqliteRecovery, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let key = key.unwrap_or_default();
    tokio::task::spawn_blocking(move || {
        sqlite_recovery::recover(&image, &node_id, parent_id.as_deref(), &key)
            .map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_sqlite_deleted_records", err))?
}

/// One page of a single SQLite table's rows, with optional column sort.
/// Reuses the preview VFS session cached by `fs_sqlite_preview` (or imports it
/// on first call). `order_by` is validated against the table's real columns
/// server-side before it touches SQL. `key` decrypts an encrypted (SQLCipher)
/// database.
#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn fs_sqlite_table_rows(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
    table: String,
    offset: i64,
    limit: i64,
    order_by: Option<String>,
    descending: bool,
    key: Option<SqliteKey>,
) -> Result<SqliteRows, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.sqlite_session.clone();
    let key = key.unwrap_or_default();
    tokio::task::spawn_blocking(move || -> Result<SqliteRows, AppError> {
        let session = sqlite_preview::cached_session(
            &mut cache.blocking_lock(),
            &image,
            &image_id,
            &node_id,
            parent_id.as_deref(),
        )
        .map_err(AppError::from)?;
        sqlite_preview::table_rows_for_session(
            &session,
            &key,
            &table,
            offset,
            limit,
            order_by.as_deref(),
            descending,
        )
        .map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_sqlite_table_rows", err))?
}

/// Execute one bounded SELECT against the database in the preview VFS.
/// 连接建立时已注册取证函数 `LC_UNIX`/`LC_UNIX_MS`/`LC_WEBKIT`/`LC_COCOA`/
/// `LC_FILETIME`/`LC_PROTO`，SQL 内可直接调用。
#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn fs_sqlite_query(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
    sql: String,
    parameters: Option<Vec<serde_json::Value>>,
    offset: u32,
    limit: u32,
    key: Option<SqliteKey>,
) -> Result<SqliteQueryPage, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.sqlite_session.clone();
    tokio::task::spawn_blocking(move || {
        let session = sqlite_preview::cached_session(
            &mut cache.blocking_lock(),
            &image,
            &image_id,
            &node_id,
            parent_id.as_deref(),
        )?;
        sqlite_preview::query_session(
            &session,
            &key.unwrap_or_default(),
            &sql,
            parameters.as_deref().unwrap_or_default(),
            offset,
            limit,
        )
        .map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_sqlite_query", err))?
}

/// Search across all tables in a SQLite database by substring or regex.
#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn fs_sqlite_search(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
    pattern: String,
    is_regex: Option<bool>,
    limit: Option<usize>,
    key: Option<SqliteKey>,
) -> Result<SqliteSearchResult, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.sqlite_session.clone();
    let key = key.unwrap_or_default();
    let is_regex = is_regex.unwrap_or(false);
    let limit = limit.unwrap_or(200);
    tokio::task::spawn_blocking(move || -> Result<SqliteSearchResult, AppError> {
        let session = sqlite_preview::cached_session(
            &mut cache.blocking_lock(),
            &image,
            &image_id,
            &node_id,
            parent_id.as_deref(),
        )
        .map_err(AppError::from)?;
        sqlite_preview::search_for_session(&session, &key, &pattern, is_regex, limit)
            .map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_sqlite_search", err))?
}

/// Realm 解析需要随机访问完整文件；不做前缀截断解析，避免把上限造成的缺页误判为
/// 真实库损坏。256 MiB 覆盖常见移动端 App Realm 库，同时避免原类型视图常驻过大内存。
const REALM_READ_CAP: usize = 256 * 1024 * 1024;

fn realm_bytes_cached(
    image: &Arc<EvidenceSession>,
    cache: &mut Option<crate::state::RealmBytesCache>,
    image_id: &str,
    node_id: &str,
) -> Result<(Arc<[u8]>, u64), AppError> {
    let key = NodeRef::new(image_id, node_id);
    if let Some(entry) = cache.as_ref() {
        if entry.key == key {
            return Ok((entry.bytes.clone(), entry.file_size));
        }
    }

    let mut session = fs_tree::FsSession::new(image);
    let stat = session.file_stat(node_id).map_err(AppError::from)?;
    if stat.size > REALM_READ_CAP as u64 {
        return Err(AppError::resource_limit(format!(
            "Realm 文件大于 {} MiB 原类型预览上限；为避免误读半截数据库，请导出后用专用工具查看。",
            REALM_READ_CAP / (1024 * 1024)
        )));
    }

    let data = session
        .read_file_limited(node_id, REALM_READ_CAP)
        .map_err(AppError::from)?;
    let bytes: Arc<[u8]> = Arc::from(data.into_boxed_slice());
    *cache = Some(crate::state::RealmBytesCache {
        key,
        bytes: bytes.clone(),
        file_size: stat.size,
    });
    Ok((bytes, stat.size))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_realm_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
) -> Result<RealmDigest, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.realm_session.clone();
    tokio::task::spawn_blocking(move || -> Result<RealmDigest, AppError> {
        let (bytes, file_size) = {
            let mut guard = cache.blocking_lock();
            realm_bytes_cached(&image, &mut guard, &image_id, &node_id)?
        };
        realm_preview::preview(bytes, file_size, false).map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_realm_preview", err))?
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_realm_table_rows(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    table_number: usize,
    offset: usize,
    limit: usize,
) -> Result<RealmRows, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.realm_session.clone();
    tokio::task::spawn_blocking(move || -> Result<RealmRows, AppError> {
        let (bytes, _file_size) = {
            let mut guard = cache.blocking_lock();
            realm_bytes_cached(&image, &mut guard, &image_id, &node_id)?
        };
        realm_preview::table_rows(bytes, table_number, offset, limit).map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_realm_table_rows", err))?
}

/// EVTX 文件原类型预览读取上限。256 MiB 覆盖常见 Windows 事件日志；
/// 超出的提示使用分析插件或专用取证工具。
const EVTX_READ_CAP: usize = 256 * 1024 * 1024;

fn ensure_evtx_cached<'a>(
    image: &Arc<EvidenceSession>,
    cache: &'a mut Option<crate::state::EvtxBytesCache>,
    image_id: &str,
    node_id: &str,
) -> Result<&'a crate::state::EvtxBytesCache, AppError> {
    let key = format!("{image_id}\u{1f}{node_id}");
    let is_match = match cache.as_ref() {
        Some(c) => c.key == key,
        None => false,
    };
    if !is_match {
        let mut session = fs_tree::FsSession::new(image);
        let stat = session.file_stat(node_id).map_err(AppError::from)?;
        if stat.size > EVTX_READ_CAP as u64 {
            return Err(AppError::resource_limit(format!(
                "EVTX 文件大于 {} MiB 原类型预览上限；建议使用 Winevtx 插件或导出后查看。",
                EVTX_READ_CAP / (1024 * 1024)
            )));
        }

        let data = session
            .read_file_limited(node_id, EVTX_READ_CAP)
            .map_err(AppError::from)?;
        let bytes: Arc<[u8]> = Arc::from(data.into_boxed_slice());
        let (digest, records) = evtx_preview::parse_evtx(bytes.clone(), stat.size)
            .map_err(|e| AppError::internal(e.to_string()))?;

        *cache = Some(crate::state::EvtxBytesCache {
            key,
            bytes,
            file_size: stat.size,
            digest,
            records,
        });
    }

    cache
        .as_ref()
        .ok_or_else(|| AppError::internal("evtx cache not initialized"))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_evtx_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
) -> Result<EvtxDigest, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.evtx_session.clone();
    tokio::task::spawn_blocking(move || -> Result<EvtxDigest, AppError> {
        let mut guard = cache.blocking_lock();
        let entry = ensure_evtx_cached(&image, &mut guard, &image_id, &node_id)?;
        Ok(entry.digest.clone())
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_evtx_preview", err))?
}

#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn fs_evtx_records(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    query: Option<String>,
    level: Option<u8>,
    event_id: Option<u32>,
    offset: usize,
    limit: usize,
) -> Result<EvtxPage, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.evtx_session.clone();
    tokio::task::spawn_blocking(move || -> Result<EvtxPage, AppError> {
        let mut guard = cache.blocking_lock();
        let entry = ensure_evtx_cached(&image, &mut guard, &image_id, &node_id)?;
        let page = evtx_preview::filter_records(
            &entry.records,
            query.as_deref(),
            level,
            event_id,
            offset,
            limit.clamp(1, 1000),
        );
        Ok(page)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_evtx_records", err))?
}

/// 二进制结构解析的读取上限。头部/导入导出/版本资源都在文件前部，但节区熵需要
/// 节区原始数据 —— 64 MiB 覆盖绝大多数可执行文件；超出的标记 truncated。
const BINARY_READ_CAP: usize = 64 * 1024 * 1024;

/// 解析镜像内文件的二进制结构（ELF / Mach-O / PE / Unix ar，自动按魔数分派），
/// 供「原类型视图」渲染。节点须是受支持的二进制，否则返回解释性错误字符串。
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_binary_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
) -> Result<crate::core::preview::binary::binfmt::NativeReport, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    tokio::task::spawn_blocking(
        move || -> Result<crate::core::preview::binary::binfmt::NativeReport, AppError> {
            let mut session = fs_tree::FsSession::new(&image);
            let stat = session.file_stat(&node_id).map_err(AppError::from)?;
            let data = session
                .read_file_limited(&node_id, BINARY_READ_CAP)
                .map_err(AppError::from)?;
            crate::core::preview::binary::binfmt::parse(&data, stat.size.max(data.len() as u64))
                .map_err(AppError::from)
        },
    )
    .await
    .map_err(|err| AppError::task_panicked("fs_binary_preview", err))?
}

/// GGUF 解析的读取上限。头部 + 元数据 + 张量信息都在文件前部（即便十万级 token
/// 词表通常也只占数 MB），权重数据本体不读 —— 64 MiB 远超头区所需。
const GGUF_READ_CAP: usize = 64 * 1024 * 1024;

/// 解析镜像内文件的 GGUF 结构，供「原类型视图」渲染（节点须是 GGUF 文件，
/// 否则返回解释性错误字符串）。
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_gguf_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
) -> Result<crate::core::preview::gguf::GgufReport, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    tokio::task::spawn_blocking(
        move || -> Result<crate::core::preview::gguf::GgufReport, AppError> {
            let mut session = fs_tree::FsSession::new(&image);
            let stat = session.file_stat(&node_id).map_err(AppError::from)?;
            let data = session
                .read_file_limited(&node_id, GGUF_READ_CAP)
                .map_err(AppError::from)?;
            crate::core::preview::gguf::parse(&data, stat.size.max(data.len() as u64))
                .map_err(AppError::from)
        },
    )
    .await
    .map_err(|err| AppError::task_panicked("fs_gguf_preview", err))?
}

/// ZIP 结构解析的读取上限。压缩包的中央目录在文件**尾部**，而文件节点的读取原语
/// 没有按偏移随机读，故须把整包载入内存才能定位中央目录 —— 128 MiB 对齐应用既有
/// 「整文件读取」上限，覆盖绝大多数取证压缩包；超出的在尾部中央目录不可达时报错。
/// 成员内容点击复用 `AppState::zip_session` 缓存的这同一段字节，不再整包重读。
const ZIP_READ_CAP: usize = 128 * 1024 * 1024;

/// 成员内容预览的单次解压上限：文本/十六进制预览只看头部，1 MiB 足够且避免把
/// 多 GiB 成员整体解压进内存。
const ZIP_MEMBER_READ_CAP: usize = 1024 * 1024;

/// 取得（或填充）某个镜像内 ZIP 节点的整包字节，命中单槽缓存则直接复用。返回
/// `(字节, 是否因超上限被截断)`。仅在 `spawn_blocking` 内、持有 `zip_session`
/// 锁时调用。
fn zip_bytes_cached(
    image: &Arc<EvidenceSession>,
    cache: &mut Option<crate::state::ZipBytesCache>,
    image_id: &str,
    node_id: &str,
) -> Result<(Arc<Vec<u8>>, bool), AppError> {
    let key = NodeRef::new(image_id, node_id);
    if let Some(entry) = cache.as_ref() {
        if entry.key == key {
            return Ok((entry.bytes.clone(), entry.truncated));
        }
    }
    let mut session = fs_tree::FsSession::new(image);
    let stat = session.file_stat(node_id).map_err(AppError::from)?;
    let data = session
        .read_file_limited(node_id, ZIP_READ_CAP)
        .map_err(AppError::from)?;
    let truncated = stat.size > data.len() as u64;
    let bytes = Arc::new(data);
    *cache = Some(crate::state::ZipBytesCache {
        key,
        bytes: bytes.clone(),
        truncated,
    });
    Ok((bytes, truncated))
}

/// 解析镜像内 ZIP 文件的内部结构（中央目录），供「原类型视图」的目录树渲染。
/// 节点须是有效 ZIP，否则返回解释性错误字符串。
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_zip_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
) -> Result<crate::core::preview::zip_preview::ZipPreview, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.zip_session.clone();
    tokio::task::spawn_blocking(move || -> Result<crate::core::preview::zip_preview::ZipPreview, AppError> {
        // 只在取字节时持锁，随后释放，避免解析期间阻塞另一格式的预览。
        let (bytes, truncated) = {
            let mut guard = cache.blocking_lock();
            zip_bytes_cached(&image, &mut guard, &image_id, &node_id)?
        };
        match crate::core::preview::zip_preview::parse(&bytes, bytes.len() as u64, truncated) {
            Ok(report) => Ok(report),
            Err(err) if truncated => {
                tracing::warn!("fs_zip_preview truncated parse failed: {err:#}");
                Err(AppError::resource_limit(format!(
                    "压缩包大于 {} MiB 结构解析上限，尾部中央目录不可达，无法读取内部结构；请导出该文件后查看。",
                    ZIP_READ_CAP / (1024 * 1024)
                )))
            }
            Err(err) => Err(AppError::from(err)),
        }
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_zip_preview", err))?
}

/// 解压镜像内 ZIP 文件的某一个成员，回传其内容（限 1 MiB）作文本/十六进制预览。
/// 原始字节经 `crate::host::Response` 回传（前端收 `ArrayBuffer`），避免 JSON 整数
/// 数组膨胀。目录/符号链接/加密/不支持压缩方法的成员返回解释性错误。
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_zip_member_read(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    member_path: String,
    max_bytes: usize,
) -> Result<crate::host::Response, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.zip_session.clone();
    let data = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, AppError> {
        let (bytes, _truncated) = {
            let mut guard = cache.blocking_lock();
            zip_bytes_cached(&image, &mut guard, &image_id, &node_id)?
        };
        crate::core::preview::zip_preview::read_member(
            &bytes,
            &member_path,
            max_bytes.min(ZIP_MEMBER_READ_CAP),
        )
        .map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_zip_member_read", err))??;
    Ok(crate::host::Response::new(data))
}

/// CSV / TSV 解析的读取上限。表格视图在内存里全量持有解析结果（含前端排序/
/// 统计），故读取窗口须有界 —— 16 MiB 覆盖绝大多数日志 / 导出表；超出的标记
/// truncated。返回行数另有 `csv_table::MAX_ROWS` 上限。
const CSV_READ_CAP: usize = 16 * 1024 * 1024;

/// 解析镜像内文件的 CSV / TSV 结构，供「原类型视图」表格渲染。`delimiter` 为
/// None / 空 / "auto" 时自动嗅探分隔符，否则按给定分隔符解析（前端切换分隔符时
/// 重新调用）。解析在后端，前端只负责渲染与搜索/排序交互。
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_csv_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    delimiter: Option<String>,
) -> Result<crate::core::preview::tables::csv_table::CsvTable, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    tokio::task::spawn_blocking(
        move || -> Result<crate::core::preview::tables::csv_table::CsvTable, AppError> {
            let mut session = fs_tree::FsSession::new(&image);
            let stat = session.file_stat(&node_id).map_err(AppError::from)?;
            let data = session
                .read_file_limited(&node_id, CSV_READ_CAP)
                .map_err(AppError::from)?;
            let truncated_bytes = stat.size > data.len() as u64;
            let delim =
                crate::core::preview::tables::csv_table::delimiter_from_str(delimiter.as_deref());
            Ok(crate::core::preview::tables::csv_table::parse(
                &data,
                delim,
                truncated_bytes,
            ))
        },
    )
    .await
    .map_err(|err| AppError::task_panicked("fs_csv_preview", err))?
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_vim_swap_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
) -> Result<VimSwapDigest, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    tokio::task::spawn_blocking(move || {
        vim_swap::preview_vim_swap(&image, &node_id).map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_vim_swap_preview", err))?
}

const REGISTRY_SIDECAR_CAP: usize = 128 * 1024 * 1024;

fn infer_ntfs_parent_id(image: &Arc<EvidenceSession>, node_id: &str) -> Option<String> {
    let (source, record) = parse_ntfs_id(node_id)?;
    let vol = fs_tree::open_ntfs_for_source(image, source).ok()?;
    let meta = vol.record_metadata(record).ok()?;
    let parent_record = meta
        .filenames
        .iter()
        .max_by_key(|f| mft_preview::namespace_priority(f.namespace))
        .map(|f| f.parent_record)?;
    Some(match source {
        NtfsSource::Partition(p) => {
            if parent_record == 5 {
                format!("part:{p}")
            } else {
                format!("ntfs:part:{p}:{parent_record}")
            }
        }
        NtfsSource::Lvm { part_id, lv_index } => {
            if parent_record == 5 {
                format!("lvm:{part_id}:{lv_index}")
            } else {
                format!("ntfs:lvm:{part_id}:{lv_index}:{parent_record}")
            }
        }
    })
}

fn find_registry_sidecars(
    image: &Arc<EvidenceSession>,
    node_id: &str,
    parent_id: Option<&str>,
) -> (String, Vec<(String, Vec<u8>)>) {
    let parent = match parent_id {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => infer_ntfs_parent_id(image, node_id).unwrap_or_else(|| {
            let trimmed = node_id.trim_end_matches('/');
            trimmed
                .rfind('/')
                .map(|idx| trimmed[..idx].to_string())
                .unwrap_or_default()
        }),
    };
    let Ok(siblings) = fs_tree::children_for_image(image, &parent) else {
        return (String::new(), Vec::new());
    };
    let Some(target) = siblings.iter().find(|s| s.id == node_id) else {
        return (String::new(), Vec::new());
    };
    let target_name = target.name.clone();
    let mut session = fs_tree::FsSession::new(image);

    let mut load_suffix = |suffix: &str| -> Option<(String, Vec<u8>)> {
        let wanted = format!("{target_name}{suffix}");
        let node = siblings.iter().find(|s| {
            s.kind == fs_tree::NodeKind::File && s.name.eq_ignore_ascii_case(&wanted) && s.size > 0
        })?;
        let bytes = session
            .read_file_limited(&node.id, REGISTRY_SIDECAR_CAP)
            .ok()?;
        (!bytes.is_empty()).then(|| (node.name.clone(), bytes))
    };

    let mut sidecars = Vec::new();
    for suffix in [".LOG1", ".LOG2"] {
        if let Some(item) = load_suffix(suffix) {
            sidecars.push(item);
        }
    }
    if sidecars.is_empty() {
        if let Some(item) = load_suffix(".LOG") {
            sidecars.push(item);
        }
    }
    (target_name, sidecars)
}

fn ensure_registry_cached<'a>(
    image: &Arc<EvidenceSession>,
    cache: &'a mut Option<crate::state::RegistrySessionCache>,
    image_id: &str,
    node_id: &str,
    parent_id: Option<&str>,
) -> Result<&'a mut crate::state::RegistrySessionCache, AppError> {
    let key = format!("{image_id}\u{1f}{node_id}");
    let is_match = cache.as_ref().is_some_and(|c| c.key == key);
    if !is_match {
        let mut session = fs_tree::FsSession::new(image);
        let stat = session.file_stat(node_id).map_err(AppError::from)?;
        if stat.size > MAX_REGISTRY_BYTES as u64 {
            return Err(AppError::resource_limit(format!(
                "注册表配置单元大于 {} MiB 原类型预览上限；请导出后用专用工具查看。",
                MAX_REGISTRY_BYTES / (1024 * 1024)
            )));
        }
        let primary_bytes = session
            .read_file_limited(node_id, MAX_REGISTRY_BYTES)
            .map_err(AppError::from)?;
        let (file_name, sidecars) = find_registry_sidecars(image, node_id, parent_id);
        let (parser, digest) =
            registry_preview::open_registry_hive(primary_bytes, stat.size, &file_name, sidecars)
                .map_err(|e| AppError::internal(e.to_string()))?;
        *cache = Some(crate::state::RegistrySessionCache {
            key,
            parser,
            digest,
        });
    }
    cache
        .as_mut()
        .ok_or_else(|| AppError::internal("registry cache not initialized"))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_registry_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
) -> Result<RegistryDigest, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.registry_session.clone();
    tokio::task::spawn_blocking(move || -> Result<RegistryDigest, AppError> {
        let mut guard = cache.blocking_lock();
        let entry = ensure_registry_cached(
            &image,
            &mut guard,
            &image_id,
            &node_id,
            parent_id.as_deref(),
        )?;
        Ok(entry.digest.clone())
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_registry_preview", err))?
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_registry_key(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
    key_path: String,
    cell_offset: Option<u64>,
) -> Result<RegistryKeyDetail, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.registry_session.clone();
    tokio::task::spawn_blocking(move || -> Result<RegistryKeyDetail, AppError> {
        let mut guard = cache.blocking_lock();
        let entry = ensure_registry_cached(
            &image,
            &mut guard,
            &image_id,
            &node_id,
            parent_id.as_deref(),
        )?;
        registry_preview::read_key_detail(&mut entry.parser, &key_path, cell_offset)
            .map_err(|e| AppError::internal(e.to_string()))
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_registry_key", err))?
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_registry_search(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
    query: String,
    limit: Option<usize>,
) -> Result<RegistrySearchResult, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.registry_session.clone();
    tokio::task::spawn_blocking(move || -> Result<RegistrySearchResult, AppError> {
        let mut guard = cache.blocking_lock();
        let entry = ensure_registry_cached(
            &image,
            &mut guard,
            &image_id,
            &node_id,
            parent_id.as_deref(),
        )?;
        Ok(registry_preview::search_hive(
            &mut entry.parser,
            &query,
            limit.unwrap_or(200),
        ))
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_registry_search", err))?
}

fn ensure_mft_cached<'a>(
    session: &mut fs_tree::FsSession<'_>,
    cache: &'a mut Option<MftSessionCache>,
    image_id: &str,
    node_id: &str,
) -> Result<&'a mut MftSessionCache, AppError> {
    let key = format!("{image_id}\u{1f}{node_id}");
    let hit = cache.as_ref().is_some_and(|entry| entry.key == key);
    if !hit {
        let stat = session.file_stat(node_id).map_err(AppError::from)?;
        let (record_size, total_records, digest) =
            mft_preview::open_mft_digest(session, node_id, stat.size)
                .map_err(|e| AppError::internal(e.to_string()))?;
        *cache = Some(MftSessionCache {
            key,
            node_id: node_id.to_string(),
            file_size: stat.size,
            record_size,
            total_records,
            digest,
            parent_cache: std::collections::HashMap::new(),
            full_index: None,
        });
    }
    cache
        .as_mut()
        .ok_or_else(|| AppError::internal("mft cache not initialized"))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_mft_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
) -> Result<MftDigest, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.mft_session.clone();
    tokio::task::spawn_blocking(move || -> Result<MftDigest, AppError> {
        let mut session = fs_tree::FsSession::new(&image);
        let mut guard = cache.blocking_lock();
        let entry = ensure_mft_cached(&mut session, &mut guard, &image_id, &node_id)?;
        Ok(entry.digest.clone())
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_mft_preview", err))?
}

#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn fs_mft_records(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    query: Option<String>,
    filter: Option<String>,
    offset: usize,
    limit: usize,
) -> Result<MftPage, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.mft_session.clone();
    tokio::task::spawn_blocking(move || -> Result<MftPage, AppError> {
        let mut session = fs_tree::FsSession::new(&image);
        let mut guard = cache.blocking_lock();
        let entry = ensure_mft_cached(&mut session, &mut guard, &image_id, &node_id)?;
        mft_preview::list_mft_records(
            &mut session,
            entry,
            query.as_deref(),
            filter.as_deref(),
            offset,
            limit.clamp(1, 500),
        )
        .map_err(|e| AppError::internal(e.to_string()))
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_mft_records", err))?
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_mft_record_detail(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    record_number: u64,
) -> Result<MftRecordDetail, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let cache = state.mft_session.clone();
    tokio::task::spawn_blocking(move || -> Result<MftRecordDetail, AppError> {
        let mut session = fs_tree::FsSession::new(&image);
        let mut guard = cache.blocking_lock();
        let entry = ensure_mft_cached(&mut session, &mut guard, &image_id, &node_id)?;
        mft_preview::read_mft_record_detail(&mut session, entry, record_number)
            .map_err(|e| AppError::internal(e.to_string()))
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_mft_record_detail", err))?
}

/// Inspect raw NTFS `$STANDARD_INFORMATION` (`$SI`) and `$FILE_NAME` (`$FN`) metadata for any
/// `ntfs:...` or `ntfs-ads:...` file node; returns `None` for non-NTFS nodes or index-only sessions.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_ntfs_record_metadata(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
) -> Result<Option<NtfsRecordForensicView>, AppError> {
    let parsed = parse_ntfs_id(&node_id)
        .or_else(|| parse_ntfs_ads_id(&node_id).map(|(src, rec, _)| (src, rec)));
    let Some((source, record)) = parsed else {
        return Ok(None);
    };
    let image = loaded_image_app_error(&state, &image_id).await?;
    if image.is_index_only() {
        return Ok(None);
    }
    tokio::task::spawn_blocking(
        move || -> Result<Option<NtfsRecordForensicView>, AppError> {
            let volume = fs_tree::open_ntfs_for_source(&image, source).map_err(AppError::from)?;
            let raw = volume.record_metadata(record).map_err(AppError::from)?;
            Ok(Some(build_ntfs_record_forensic_view(&raw)))
        },
    )
    .await
    .map_err(|err| AppError::task_panicked("fs_ntfs_record_metadata", err))?
}

/// Inspect a PostgreSQL `PGDATA` directory or standalone 8 KiB heap table file.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_pgdata_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
) -> Result<crate::core::preview::pgdata::PgDigest, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let parent_id = parent_id
        .map(|id| id.to_string())
        .or_else(|| infer_ntfs_parent_id(&image, &node_id));
    tokio::task::spawn_blocking(move || {
        crate::core::preview::pgdata::preview_pgdata(&image, &node_id, parent_id.as_deref())
            .map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_pgdata_preview", err))?
}

/// Read rows (including dead/deleted tuples when `include_dead` is true) from a PostgreSQL table.
#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn fs_pgdata_rows(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
    table_node_id: Option<NodeId>,
    include_dead: Option<bool>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<crate::core::preview::pgdata::PgTableRows, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let parent_id = parent_id
        .map(|id| id.to_string())
        .or_else(|| infer_ntfs_parent_id(&image, &node_id));
    let include_dead = include_dead.unwrap_or(true);
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(200);
    tokio::task::spawn_blocking(move || {
        crate::core::preview::pgdata::read_pgdata_table_rows(
            &image,
            &node_id,
            parent_id.as_deref(),
            table_node_id.as_deref(),
            include_dead,
            offset,
            limit,
        )
        .map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_pgdata_rows", err))?
}

/// Parse a LevelDB / Chromium IndexedDB directory or `.ldb`/`.sst`/`.log` file.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_leveldb_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
    query: Option<String>,
    include_deleted: Option<bool>,
    limit: Option<usize>,
) -> Result<crate::core::preview::leveldb::LevelDbDigest, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let parent_id = parent_id
        .map(|id| id.to_string())
        .or_else(|| infer_ntfs_parent_id(&image, &node_id));
    let include_deleted = include_deleted.unwrap_or(true);
    let limit = limit.unwrap_or(300);
    tokio::task::spawn_blocking(move || {
        crate::core::preview::leveldb::preview_leveldb(
            &image,
            &node_id,
            parent_id.as_deref(),
            query.as_deref(),
            include_deleted,
            limit,
        )
        .map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_leveldb_preview", err))?
}

/// Parse a binary `systemd-journal` (`.journal`, magic `LPKSHHRH`) file.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn fs_journal_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    node_id: NodeId,
    query: Option<String>,
    priority_max: Option<u8>,
    limit: Option<usize>,
) -> Result<crate::core::preview::journal::JournalDigest, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let limit = limit.unwrap_or(300);
    tokio::task::spawn_blocking(move || {
        crate::core::preview::journal::preview_systemd_journal(
            &image,
            &node_id,
            query.as_deref(),
            priority_max,
            limit,
        )
        .map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("fs_journal_preview", err))?
}
