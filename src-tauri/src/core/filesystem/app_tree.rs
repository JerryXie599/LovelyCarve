//! 「应用」虚拟目录 —— iOS 文件夹检材的应用容器聚合视图。
//!
//! iOS 文件系统镜像里，一个应用的数据分散在多个容器目录：数据容器
//! （`var/mobile/Applications/<bundle>`、`var/mobile/Containers/Data/Application/…`）
//! 与组容器（`group.<bundle>` / `<TeamID>.groups.<bundle>` 命名）。本模块把
//! 它们聚合成一棵挂在文件树根的虚拟「应用」子树：
//!
//! ```text
//! apps                     → 应用列表（名称后括号 = 文件总数）
//!   app:<bundle>           → 该应用全部容器（括号 = 容器内文件数）
//!     appdir:<hex(rel)>    → 容器内目录（括号 = 子树文件数）+ 真实文件节点
//! ```
//!
//! 文件节点是真实 `folder:` 节点，预览 / 定位 / 导出走原生管道。虚拟节点由
//! 命令层（`fs_children` / `fs_children_page` / `fs_flat_page`）应答，不进
//! 快照与 filedb 索引，也不参与「定位源文件」等按 id 回溯的机制。
//!
//! 计数语义：文件数 = 子树内非目录条目（文件 + 符号链接），不含目录本身；
//! 符号链接目录不递归（与实时浏览的只读语义一致）。

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::core::evidence::readers::folder_image::{FolderEntryKind, FolderImage};
use crate::core::filesystem::tree::fs_tree::{FsNode, Mactb, NodeKind};
use crate::core::filesystem::volumes::folder;

/// 文件树根部的「应用」虚拟节点 id（由前端在根列表头插入，后端应答其子级）。
pub const APPS_ROOT_ID: &str = "apps";

/// 「应用」入口节点在结果里的显示名（与前端树节点同名）。
pub const APPS_ROOT_NAME: &str = "应用";

/// 无法归属到任何 bundle 的容器（UUID 目录且无 metadata plist）聚成的伪应用名。
pub const UNATTRIBUTED_APP: &str = "未归属容器";

/// 索引里最多记录的文件条数；超出后 `truncated` 置位，计数照常累加。
pub const MAX_INDEX_FILES: usize = 500_000;

/// 应用容器可能出现的目录（按检材实况取存在的那些）。
const CONTAINER_ROOTS: &[&str] = &[
    "var/mobile/Applications",
    "var/mobile/Containers/Data/Application",
    "var/containers/Data/Application",
    "var/mobile/Containers/Bundle/Application",
    "var/mobile/Containers/Shared/AppGroup",
    "var/containers/Shared/AppGroup",
    "var/mobile/Containers/Group",
];

const MCM_PLIST: &str = ".com.apple.mobile_container_manager.metadata.plist";
const GROUP_PREFIX: &str = "group.";
const GROUPS_INFIX: &str = ".groups.";

/// 判断一个节点 id 是否落在「应用」虚拟命名空间里。
pub fn is_app_node_id(id: &str) -> bool {
    id == APPS_ROOT_ID || id.starts_with("app:") || id.starts_with("appdir:")
}

/// `appdir:<hex(rel)>` 的 mint / 还原；rel 为检材根的相对路径。
pub fn appdir_id(rel: &str) -> String {
    format!("appdir:{}", hex::encode(rel))
}

pub fn parse_appdir_id(id: &str) -> Option<String> {
    let rest = id.strip_prefix("appdir:")?;
    let bytes = hex::decode(rest).ok()?;
    String::from_utf8(bytes).ok()
}

/// 容器目录名 → `(bundle_id, 容器种类)`。种类：`data` = 数据/束容器，
/// `group` = 组容器。`None` = 无法从名字归属（UUID 目录，交给 metadata plist）。
pub fn classify_container(name: &str) -> Option<(String, &'static str)> {
    if let Some(rest) = name.strip_prefix(GROUP_PREFIX) {
        if !rest.is_empty() {
            return Some((rest.to_string(), "group"));
        }
    }
    if let Some(idx) = name.find(GROUPS_INFIX) {
        let bundle = &name[idx + GROUPS_INFIX.len()..];
        if !bundle.is_empty() {
            return Some((bundle.to_string(), "group"));
        }
    }
    if name.contains('.') {
        return Some((name.to_string(), "data"));
    }
    None
}

/// UUID 容器的归属：读容器内 `.com.apple.mobile_container_manager.metadata.plist`
/// 的 `MCMMetadataIdentifier`（真实检材个别 plist 越界/损坏，解析失败按缺失处理）。
fn uuid_bundle_id(folder: &FolderImage, rel: &str) -> Option<String> {
    let bytes = folder.read(&format!("{rel}/{MCM_PLIST}"), 1 << 20).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let parsed = catch_unwind(AssertUnwindSafe(|| {
        plist::Value::from_reader(std::io::Cursor::new(&bytes)).ok()
    }))
    .ok()
    .flatten()?;
    let identifier = parsed
        .as_dictionary()?
        .get("MCMMetadataIdentifier")
        .and_then(plist::Value::as_string)?;
    (!identifier.is_empty()).then(|| identifier.to_string())
}

/// 检材里实际存在的应用容器根目录（相对路径）。没有文件夹检材或一个都
/// 不存在时返回空 —— 命令层据此判断「应用」节点是否可用。
pub fn container_roots(folder: &FolderImage) -> Vec<String> {
    CONTAINER_ROOTS
        .iter()
        .filter(|root| {
            folder
                .lookup(root)
                .is_some_and(|entry| entry.kind == FolderEntryKind::Dir)
        })
        .map(|root| (*root).to_string())
        .collect()
}

/// 一个 bundle 聚合下的全部容器。
#[derive(Debug, Clone)]
pub struct ContainerRef {
    /// 检材根的相对路径（appdir id 的编码源）。
    pub rel: String,
    /// 容器目录名（显示用）。
    pub name: String,
    /// `data` | `group` | `unattributed`。
    pub kind: &'static str,
    pub file_count: u64,
}

/// 一个应用的聚合条目（数据容器 + 组容器）。
#[derive(Debug, Clone)]
pub struct AppEntry {
    pub bundle_id: String,
    pub containers: Vec<ContainerRef>,
    pub file_count: u64,
}

/// 全显行：真实文件节点 + 检材根相对路径（flat 的 path 字段与过滤来源）。
#[derive(Debug, Clone)]
pub struct IndexedFile {
    pub rel: String,
    pub node: FsNode,
}

/// 一次构建的应用聚合索引，按检材缓存（`AppState::app_indexes`）。
#[derive(Debug, Default)]
pub struct AppIndex {
    /// 按文件数降序、bundle id 升序。
    pub apps: Vec<AppEntry>,
    /// 容器内每个目录（含容器根本身，rel 为键）的子树文件数。
    pub dir_counts: HashMap<String, u64>,
    /// 容器内全部文件（发现序；`truncated` 时可能不满额）。
    pub files: Vec<IndexedFile>,
    pub total_files: u64,
    pub truncated: bool,
}

impl AppIndex {
    pub fn app(&self, bundle_id: &str) -> Option<&AppEntry> {
        self.apps.iter().find(|app| app.bundle_id == bundle_id)
    }
}

/// 遍历一个容器目录，累记每个子目录的子树文件数与全部文件行。返回该目录
/// 的子树文件数。
fn walk_dir(
    folder: &FolderImage,
    rel: &str,
    dir_counts: &mut HashMap<String, u64>,
    files: &mut Vec<IndexedFile>,
    total: &mut u64,
    truncated: &mut bool,
) -> u64 {
    let mut count = 0u64;
    for entry in folder.children(rel) {
        let child_rel = entry.rel_path.clone();
        match entry.kind {
            FolderEntryKind::Dir => {
                count += walk_dir(folder, &child_rel, dir_counts, files, total, truncated);
            }
            _ => {
                count += 1;
                *total += 1;
                if files.len() < MAX_INDEX_FILES {
                    files.push(IndexedFile {
                        rel: child_rel,
                        node: folder::entry_to_node(entry),
                    });
                } else {
                    *truncated = true;
                }
            }
        }
    }
    dir_counts.insert(rel.to_string(), count);
    count
}

/// 构建应用聚合索引。`roots` 来自 [`container_roots`]。
pub fn build_index(folder: &FolderImage, roots: &[String]) -> AppIndex {
    let mut by_app: HashMap<String, Vec<ContainerRef>> = HashMap::new();
    let mut dir_counts = HashMap::new();
    let mut files = Vec::new();
    let mut total = 0u64;
    let mut truncated = false;

    for root in roots {
        for entry in folder.children(root) {
            if entry.kind != FolderEntryKind::Dir {
                continue;
            }
            let rel = entry.rel_path.clone();
            let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
            let (bundle, kind) = match classify_container(&name) {
                Some(attributed) => attributed,
                None => match uuid_bundle_id(folder, &rel) {
                    Some(id) => (id, "data"),
                    None => (UNATTRIBUTED_APP.to_string(), "unattributed"),
                },
            };
            let file_count = walk_dir(
                folder,
                &rel,
                &mut dir_counts,
                &mut files,
                &mut total,
                &mut truncated,
            );
            let containers = by_app.entry(bundle).or_default();
            containers.push(ContainerRef {
                rel,
                name,
                kind,
                file_count,
            });
        }
    }

    let mut apps: Vec<AppEntry> = by_app
        .into_iter()
        .map(|(bundle_id, containers)| {
            let file_count = containers.iter().map(|c| c.file_count).sum();
            AppEntry {
                bundle_id,
                containers,
                file_count,
            }
        })
        .collect();
    apps.sort_by(|a, b| {
        b.file_count
            .cmp(&a.file_count)
            .then_with(|| a.bundle_id.cmp(&b.bundle_id))
    });
    for app in &mut apps {
        app.containers.sort_by(|a, b| {
            container_kind_rank(a.kind)
                .cmp(&container_kind_rank(b.kind))
                .then_with(|| a.name.cmp(&b.name))
        });
    }

    AppIndex {
        apps,
        dir_counts,
        files,
        total_files: total,
        truncated,
    }
}

fn container_kind_rank(kind: &str) -> u8 {
    match kind {
        "data" => 0,
        "group" => 1,
        _ => 2,
    }
}

/// 千位分隔（前端原生树的计数用 `toLocaleString`，这里保持同风格）。
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (idx, ch) in digits.chars().enumerate() {
        out.push(ch);
        let remaining = len - idx - 1;
        if remaining > 0 && remaining % 3 == 0 {
            out.push(',');
        }
    }
    out
}

fn virtual_dir_node(id: String, name: String, parent: &str, file_count: u64) -> FsNode {
    FsNode {
        id,
        name: format!("{name} ({})", thousands(file_count)),
        parent_id: Some(parent.to_string()),
        kind: NodeKind::Dir,
        size: 0,
        deleted: false,
        inode: None,
        timestamps: Mactb::default(),
        link_target: None,
        has_snapshot: false,
    }
}

/// 常见应用 Bundle ID 到友好中文/通用名称的映射（美亚杯及国内外主流应用）
pub fn app_display_name(bundle_id: &str) -> Option<&'static str> {
    let lower = bundle_id.to_ascii_lowercase();
    match lower.as_str() {
        // 美亚杯及重点检材应用
        "com.quora.app.experts" => Some("Poe"),
        "com.lihkg.forum-ios" => Some("LIHKG 连登"),
        "net.whatsapp.whatsapp" => Some("WhatsApp"),
        "net.whatsapp.whatsapp.shared" => Some("WhatsApp 共享"),
        "ph.telegra.telegraph" | "org.telegram.messenger" => Some("Telegram"),
        "com.yoosh.infltr" => Some("Infltr 滤镜"),
        "com.ai.polyverse.mirror.ios" => Some("Mirror 镜像"),
        "openrice.hongkong" => Some("开饭喇 (OpenRice)"),
        "io.metamask.metamask" => Some("MetaMask 小狐狸"),
        "com.prisma-ai.app" => Some("Prisma 艺术滤镜"),
        "com.burbn.instagram" => Some("Instagram"),
        "com.xunmeng.pinduoduo" => Some("拼多多"),
        "com.meitu.mtxx" => Some("美图秀秀"),
        "com.uazoo.ssc" => Some("生生不息"),
        "com.google.chrome.ios" => Some("Chrome"),
        "com.google.gmail" => Some("Gmail"),
        "com.google.maps" => Some("Google 地图"),
        "com.google.common" => Some("Google 服务"),
        "com.google.drive" => Some("Google 云端硬盘"),
        "com.google.photos" => Some("Google 相册"),
        "com.google.youtube" => Some("YouTube"),
        "com.zhiliaoapp.musically" => Some("TikTok"),
        "com.ss.iphone.ugc.aweme" => Some("抖音"),
        "com.taobao.taobao4iphone" => Some("手机淘宝"),
        "com.alipay.iphoneclient" => Some("支付宝"),
        "com.sina.weibo" => Some("微博"),
        "com.baidu.baidusearch" => Some("百度"),
        "com.apple.appstore" => Some("App Store"),
        "com.apple.mobilesafari" => Some("Safari 浏览器"),
        "com.apple.mobilesms" => Some("信息"),
        "com.apple.mobilemail" => Some("邮件"),
        "com.apple.mobilephone" => Some("电话"),
        "com.apple.music" => Some("音乐"),
        "com.apple.ibooks" => Some("图书"),
        "com.apple.maps" => Some("地图"),
        "com.apple.freeform" => Some("无边记"),
        "com.apple.health" => Some("健康"),
        "com.apple.fitness" => Some("健身"),
        "com.apple.camera" => Some("相机"),
        "com.apple.mobileslideshow" => Some("照片"),
        "com.apple.tips" | "com.apple.tipsnext" => Some("提示"),
        "com.apple.preferences" => Some("设置"),
        "com.apple.fileprovider.localstorage" => Some("文件本地存储"),
        "com.apple.cloudphotosd" => Some("iCloud 照片"),
        "com.apple.linkd" => Some("快捷指令后台"),
        "com.apple.posterboard" => Some("壁纸海报"),
        "com.apple.screenshotsservicesservice" => Some("屏幕截图服务"),
        "com.apple.appstored" => Some("App Store 后台"),
        "com.apple.datadetectors.ddactionsservice" => Some("数据检测操作服务"),
        "com.apple.pegasusconfiguration" => Some("Pegasus 配置"),
        _ => None,
    }
}

pub fn app_display_label(bundle_id: &str) -> String {
    if bundle_id == UNATTRIBUTED_APP {
        return UNATTRIBUTED_APP.to_string();
    }
    if let Some(friendly) = app_display_name(bundle_id) {
        format!("{friendly} ({bundle_id})")
    } else {
        bundle_id.to_string()
    }
}

/// `apps` 的子级：应用列表。
pub fn apps_children(index: &AppIndex) -> Vec<FsNode> {
    index
        .apps
        .iter()
        .map(|app| {
            let label = app_display_label(&app.bundle_id);
            virtual_dir_node(
                format!("app:{}", app.bundle_id),
                label,
                APPS_ROOT_ID,
                app.file_count,
            )
        })
        .collect()
}

/// `app:<bundle>` 的子级：该应用全部容器（appdir 虚拟节点）。
pub fn app_children(index: &AppIndex, bundle_id: &str) -> Option<Vec<FsNode>> {
    let app = index.app(bundle_id)?;
    Some(
        app.containers
            .iter()
            .map(|container| {
                virtual_dir_node(
                    appdir_id(&container.rel),
                    container.name.clone(),
                    &format!("app:{bundle_id}"),
                    container.file_count,
                )
            })
            .collect(),
    )
}

/// `appdir:<hex(rel)>` 的子级：子目录（虚拟，带计数）+ 真实文件/链接节点。
pub fn dir_children(folder: &FolderImage, index: &AppIndex, rel: &str) -> Option<Vec<FsNode>> {
    if !index.dir_counts.contains_key(rel) {
        return None;
    }
    let mut dirs: Vec<FsNode> = Vec::new();
    let mut files: Vec<FsNode> = Vec::new();
    for entry in folder.children(rel) {
        match entry.kind {
            FolderEntryKind::Dir => {
                let child_rel = entry.rel_path.clone();
                let count = index.dir_counts.get(&child_rel).copied().unwrap_or(0);
                dirs.push(virtual_dir_node(
                    appdir_id(&child_rel),
                    entry
                        .rel_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&child_rel)
                        .to_string(),
                    &appdir_id(rel),
                    count,
                ));
            }
            _ => files.push(folder::entry_to_node(entry)),
        }
    }
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| a.name.cmp(&b.name));
    dirs.extend(files);
    Some(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_matches_group_and_bundle_names() {
        assert_eq!(
            classify_container("group.com.apple.podcasts"),
            Some(("com.apple.podcasts".to_string(), "group"))
        );
        assert_eq!(
            classify_container("243LU875E5.groups.com.apple.podcasts"),
            Some(("com.apple.podcasts".to_string(), "group"))
        );
        assert_eq!(classify_container("0AB1C2D3-1234"), None);
        // 退化名「group.」：无 bundle 段，落回数据容器分支（无害）。
        assert_eq!(
            classify_container("group."),
            Some(("group.".to_string(), "data"))
        );
    }

    #[test]
    fn appdir_ids_roundtrip() {
        let id = appdir_id("var/mobile/Applications/com.a");
        assert_eq!(
            parse_appdir_id(&id).as_deref(),
            Some("var/mobile/Applications/com.a")
        );
        assert!(!is_app_node_id("folder:ab12"));
        assert!(!is_app_node_id("apfs:1"));
        assert!(is_app_node_id(APPS_ROOT_ID));
        assert!(is_app_node_id("app:com.a"));
        assert!(is_app_node_id(&id));
    }

    #[test]
    fn thousands_groups_by_three() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(12_345_678), "12,345,678");
    }

    /// 端到端（内存内）：临时目录造一个 iOS 布局的文件夹检材，验证
    /// 检测 → 归属 → 计数 → 三级节点结构。
    #[test]
    fn builds_aggregate_from_folder_fixture() {
        use std::fs;
        use std::path::Path;

        let tmp = std::env::temp_dir().join(format!(
            "lc-app-tree-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let apps_root = tmp.join("var/mobile/Applications");
        fs::create_dir_all(apps_root.join("com.a/Documents")).unwrap();
        fs::create_dir_all(apps_root.join("com.a/Library")).unwrap();
        fs::create_dir_all(apps_root.join("group.com.a")).unwrap();
        fs::create_dir_all(apps_root.join("TEAM.groups.com.a/tmp")).unwrap();
        fs::write(apps_root.join("com.a/Documents/a.txt"), b"1").unwrap();
        fs::write(apps_root.join("com.a/Library/b.bin"), b"22").unwrap();
        fs::write(apps_root.join("group.com.a/c.txt"), b"333").unwrap();
        fs::write(apps_root.join("TEAM.groups.com.a/tmp/d.txt"), b"4444").unwrap();

        let folder = FolderImage::open(&tmp).unwrap();
        let roots = container_roots(&folder);
        assert_eq!(roots, vec!["var/mobile/Applications".to_string()]);

        let index = build_index(&folder, &roots);
        // 三个容器都归属 com.a：数据容器 + 两种组容器命名。
        assert_eq!(index.apps.len(), 1);
        let app = &index.apps[0];
        assert_eq!(app.bundle_id, "com.a");
        assert_eq!(app.file_count, 4);
        assert_eq!(app.containers.len(), 3);
        assert_eq!(app.containers[0].kind, "data");
        assert_eq!(app.containers[1].kind, "group");
        assert_eq!(app.containers[2].kind, "group");
        assert_eq!(index.total_files, 4);
        assert!(!index.truncated);

        // 目录计数：容器根 = 子树文件数。
        let container_rel = "var/mobile/Applications/com.a";
        assert_eq!(index.dir_counts[container_rel], 2);
        assert_eq!(index.dir_counts[&format!("{container_rel}/Documents")], 1);

        // 三级节点结构。
        let app_nodes = apps_children(&index);
        assert_eq!(app_nodes.len(), 1);
        assert_eq!(app_nodes[0].name, "com.a (4)");
        assert_eq!(app_nodes[0].id, "app:com.a");

        let container_nodes = app_children(&index, "com.a").unwrap();
        assert_eq!(container_nodes.len(), 3);
        let data_node = &container_nodes[0];
        assert_eq!(data_node.name, "com.a (2)");
        let data_rel = parse_appdir_id(&data_node.id).unwrap();
        assert_eq!(data_rel, container_rel);

        let level = dir_children(&folder, &index, &data_rel).unwrap();
        assert_eq!(level.len(), 2); // Documents + Library，无顶层文件
        assert!(level.iter().all(|node| node.id.starts_with("appdir:")));

        let docs_rel = format!("{container_rel}/Documents");
        let docs_level = dir_children(&folder, &index, &docs_rel).unwrap();
        assert_eq!(docs_level.len(), 1);
        assert_eq!(docs_level[0].kind, NodeKind::File);
        assert_eq!(
            docs_level[0].id,
            format!("folder:{}", hex::encode(format!("{docs_rel}/a.txt")))
        );

        // appdir 游标之外的防御：非容器目录不受命中。
        assert!(dir_children(&folder, &index, "var/mobile").is_none());

        fs::remove_dir_all(&tmp).ok();
        let _ = Path::new(&tmp);
    }

    /// 真实检材回归（本机路径，`cargo test -- --ignored` 手动跑）：25 美亚杯
    /// FUNG_CC 手机镜像文件夹。验证容器归属规模与计数非零。
    #[test]
    #[ignore = "本机检材路径"]
    fn real_fixture_fung_cc() {
        let root = std::path::Path::new(r"E:\电子取证\25美亚个人\镜像\FUNG_CC_mobile");
        if !root.is_dir() {
            panic!("检材不存在：{}", root.display());
        }
        let folder = FolderImage::open(root).unwrap();
        let roots = container_roots(&folder);
        assert_eq!(roots, vec!["var/mobile/Applications".to_string()]);
        let index = build_index(&folder, &roots);
        assert!(index.apps.len() >= 60, "应用数异常：{}", index.apps.len());
        assert!(
            index.total_files > 5_000,
            "文件数异常：{}",
            index.total_files
        );
        assert!(!index.truncated);
        // 组容器命名（TeamID.groups.*）真实存在于该检材。
        let has_group = index
            .apps
            .iter()
            .any(|app| app.containers.iter().any(|c| c.kind == "group"));
        assert!(has_group, "未发现任何组容器");
        tracing::info!(
            "FUNG_CC 应用数={} 文件总数={}",
            index.apps.len(),
            index.total_files
        );
    }
}
