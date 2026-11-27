//! 「应用」虚拟目录的命令层应答：`fs_children` / `fs_children_page` /
//! `fs_flat_page` 对 `apps` / `app:` / `appdir:` 命名空间的拦截入口。
//!
//! 聚合逻辑与索引在 `core::filesystem::app_tree`；本模块只负责：
//! 缓存 get-or-build（`AppState::app_indexes`，构建串行于
//! `app_index_build_lock`）+ 把父 id 翻译成对应子级。

use std::collections::HashSet;
use std::sync::Arc;

use crate::app::error::AppError;
use crate::commands::filesystem::filesystem_page::{
    fs_node_page_from_filedb, fs_node_page_from_sorted_nodes,
};
use crate::core::catalog::directory_query::{self, DirectoryQuery};
use crate::core::filesystem::app_tree::{self, AppIndex};
use crate::core::filesystem::evidence_session::EvidenceSession;
use crate::core::filesystem::tree::fs_tree::{FsNode, NodeKind};
use crate::state::AppState;

/// 取（或构建）检材的应用聚合索引。检材不是文件夹、或没有任何应用容器根
/// 目录时返回 `None`（命令层放行走原生路径）。
pub(crate) async fn app_index(
    state: &AppState,
    image_id: &str,
    image: &Arc<EvidenceSession>,
) -> Result<Option<Arc<AppIndex>>, AppError> {
    if let Some(index) = state.app_indexes.lock().await.get(image_id) {
        return Ok(Some(Arc::clone(index)));
    }
    let folder = match image.folder.as_ref() {
        Some(folder) => Arc::clone(folder),
        None => return Ok(None),
    };
    if app_tree::container_roots(&folder).is_empty() {
        return Ok(None);
    }
    // 单飞构建：并发首个请求只做一次遍历。
    let _guard = state.app_index_build_lock.lock().await;
    if let Some(index) = state.app_indexes.lock().await.get(image_id) {
        return Ok(Some(Arc::clone(index)));
    }
    let image_id_for_task = image_id.to_string();
    let built = tokio::task::spawn_blocking(move || {
        let roots = app_tree::container_roots(&folder);
        Arc::new(app_tree::build_index(&folder, &roots))
    })
    .await
    .map_err(|err| AppError::task_panicked("app_index build", err))?;
    tracing::info!(
        "app_index built image={image_id_for_task} apps={} files={} truncated={}",
        built.apps.len(),
        built.total_files,
        built.truncated
    );
    state
        .app_indexes
        .lock()
        .await
        .insert(image_id_for_task, Arc::clone(&built));
    Ok(Some(built))
}

/// 「应用」命名空间一层的子级节点；父 id 不在命名空间或无应用布局时 `None`。
async fn level_nodes(
    state: &AppState,
    image: &Arc<EvidenceSession>,
    image_id: &str,
    parent_id: &str,
) -> Option<Result<Vec<FsNode>, AppError>> {
    if !app_tree::is_app_node_id(parent_id) {
        return None;
    }
    let index = match app_index(state, image_id, image).await {
        Ok(Some(index)) => index,
        Ok(None) => return Some(Ok(Vec::new())),
        Err(err) => return Some(Err(err)),
    };
    let Some(folder) = image.folder.as_ref() else {
        return Some(Ok(Vec::new()));
    };
    Some(Ok(match parent_id {
        app_tree::APPS_ROOT_ID => app_tree::apps_children(&index),
        id => match id.strip_prefix("app:") {
            Some(bundle) => app_tree::app_children(&index, bundle).unwrap_or_default(),
            None => match app_tree::parse_appdir_id(id) {
                Some(rel) => app_tree::dir_children(&folder, &index, &rel).unwrap_or_default(),
                None => Vec::new(),
            },
        },
    }))
}

/// `fs_children('root')` 的「应用」入口节点：iOS 文件夹检材才提供。轻量探测
/// （只查容器根是否存在，不构建索引——索引延迟到首次展开「应用」或进入
/// 列表时），已建索引时名称带上文件总数。
pub(crate) async fn apps_root_node(
    state: &AppState,
    image_id: &str,
    image: &Arc<EvidenceSession>,
) -> Result<Option<FsNode>, AppError> {
    let count = if let Some(index) = state.app_indexes.lock().await.get(image_id) {
        Some(index.total_files)
    } else {
        let Some(folder) = image.folder.as_ref() else {
            return Ok(None);
        };
        if app_tree::container_roots(&folder).is_empty() {
            return Ok(None);
        }
        None
    };
    Ok(Some(FsNode {
        id: app_tree::APPS_ROOT_ID.to_string(),
        name: match count {
            Some(count) => format!(
                "{} ({})",
                app_tree::APPS_ROOT_NAME,
                app_tree::thousands(count)
            ),
            None => app_tree::APPS_ROOT_NAME.to_string(),
        },
        parent_id: None,
        kind: NodeKind::Dir,
        size: 0,
        deleted: false,
        inode: None,
        timestamps: Default::default(),
        link_target: None,
        has_snapshot: false,
    }))
}

/// 根列表统一前插「应用」入口（`apps_root_node` 为 None 时原样返回）。
pub(crate) fn with_apps_root(root: Option<&FsNode>, mut nodes: Vec<FsNode>) -> Vec<FsNode> {
    if let Some(node) = root {
        nodes.insert(0, node.clone());
    }
    nodes
}

/// `fs_children` 的拦截入口。
pub(crate) async fn try_children(
    state: &AppState,
    image: &Arc<EvidenceSession>,
    image_id: &str,
    parent_id: &str,
) -> Option<Result<Vec<FsNode>, AppError>> {
    level_nodes(state, image, image_id, parent_id).await
}

/// `fs_children_page` 的拦截入口。前端总是携带 query 对象，虚拟节点用
/// `directory_query::memory_page` 套用同一套过滤 / 排序 / 游标语义。
pub(crate) async fn try_children_page(
    state: &AppState,
    image: &Arc<EvidenceSession>,
    image_id: &str,
    parent_id: &str,
    query: Option<&DirectoryQuery>,
    cursor: Option<&str>,
    page_size: Option<usize>,
) -> Option<Result<crate::commands::filesystem::filesystem_page::FsNodePage, AppError>> {
    let nodes = match level_nodes(state, image, image_id, parent_id).await {
        Some(Ok(nodes)) => nodes,
        Some(Err(err)) => return Some(Err(err)),
        None => return None,
    };
    let page = match query {
        Some(query) => {
            let limit = crate::commands::filesystem::filesystem_page::normalize_fs_node_page_size(
                page_size,
            );
            let hidden = HashSet::new();
            match directory_query::memory_page(nodes, query, &hidden, cursor, limit) {
                Ok(page) => fs_node_page_from_filedb(page),
                Err(err) => return Some(Err(AppError::from(err))),
            }
        }
        None => match fs_node_page_from_sorted_nodes(nodes, cursor, page_size) {
            Ok(page) => page,
            Err(err) => return Some(Err(err)),
        },
    };
    Some(Ok(page))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_guard_rejects_real_ids() {
        // 非「应用」命名空间的 id 一律放行走原生路径（布局判定由
        // app_tree::container_roots 的单元测试与集成测试覆盖）。
        assert!(!app_tree::is_app_node_id("part:1"));
        assert!(!app_tree::is_app_node_id("folder:7661"));
        assert!(!app_tree::is_app_node_id("root"));
    }
}
