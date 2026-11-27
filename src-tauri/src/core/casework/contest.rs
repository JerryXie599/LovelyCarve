//! 竞赛题目、证据标注（Pin）与 Writeup（Markdown + 真实截图）持久化领域模块。
//!
//! 存放在 `<案件目录>/contest/contest.json` 与 `<案件目录>/contest/assets/*.png`，
//! 不修改 `case.db` schema，且不被 `case_heal` 清理。
//! 支持：
//! 1. 导入标准化 Markdown / JSON 赛题与 `exhibits_meta.json`，也支持零导入直接使用 Pin 与 WP；
//! 2. 检材自动/手动映射到案件中的 `EvidenceRef`；
//! 3. 题目作答、状态管理、CLI 指令/人工备注记录；
//! 4. 证据 Pin 标记（文件、插件分析记录、十六进制区间、SQLite 行）与真实桌面截图关联；
//! 5. 连续按检材组织的 Markdown Writeup 预览与导出（含 `./assets/*.png` 本地图片目录）。

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::casework::case::EvidenceRef;
use crate::core::casework::case_paths;

pub const CONTEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum QuestionStatus {
    #[default]
    Unanswered,
    Answered,
    Review,
    Stuck,
}

impl QuestionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unanswered => "unanswered",
            Self::Answered => "answered",
            Self::Review => "review",
            Self::Stuck => "stuck",
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Unanswered => "未作答",
            Self::Answered => "已作答",
            Self::Review => "待复核",
            Self::Stuck => "疑难",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "unanswered" | "todo" | "pending" | "未作答" => Some(Self::Unanswered),
            "answered" | "done" | "solved" | "已作答" => Some(Self::Answered),
            "review" | "check" | "待复核" => Some(Self::Review),
            "stuck" | "hard" | "疑难" => Some(Self::Stuck),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContestWorkspace {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default)]
    pub source_format: String,
    #[serde(default)]
    pub case_wp_md: String,
    #[serde(default)]
    pub exhibits: Vec<ContestExhibit>,
    #[serde(default)]
    pub questions: Vec<ContestQuestion>,
    #[serde(default)]
    pub pins: Vec<EvidencePin>,
    #[serde(default = "now_iso")]
    pub updated_at: String,
}

fn default_version() -> u32 {
    CONTEST_SCHEMA_VERSION
}

fn default_title() -> String {
    "竞赛做题工作台".to_string()
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

impl Default for ContestWorkspace {
    fn default() -> Self {
        Self {
            version: CONTEST_SCHEMA_VERSION,
            title: default_title(),
            source_format: String::new(),
            case_wp_md: String::new(),
            exhibits: Vec::new(),
            questions: Vec::new(),
            pins: Vec::new(),
            updated_at: now_iso(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContestExhibit {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub holder: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub device_type: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub image_id: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub notes_md: String,
    #[serde(default)]
    pub wp_md: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContestQuestion {
    pub id: String,
    pub exhibit_id: String,
    pub number: u32,
    pub stem: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub format_hint: Option<String>,
    #[serde(default)]
    pub answer: String,
    #[serde(default)]
    pub status: QuestionStatus,
    #[serde(default)]
    pub notes: Vec<QuestionNote>,
    #[serde(default)]
    pub wp_section_md: String,
    #[serde(default = "now_iso")]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionNote {
    pub id: String,
    #[serde(default = "default_author")]
    pub author: String,
    #[serde(default = "default_note_kind")]
    pub kind: String,
    #[serde(default)]
    pub command: Option<String>,
    pub content: String,
    #[serde(default = "now_iso")]
    pub created_at: String,
}

fn default_author() -> String {
    "user".to_string()
}

fn default_note_kind() -> String {
    "note".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PinTarget {
    #[serde(rename_all = "camelCase")]
    File {
        image_id: String,
        #[serde(default)]
        volume_key: Option<String>,
        #[serde(default)]
        node_id: Option<i64>,
        path: String,
        #[serde(default)]
        meta_addr: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    AnalysisRecord {
        image_id: String,
        category_id: String,
        #[serde(default)]
        category_name: Option<String>,
        record_id: i64,
        #[serde(default)]
        preview_text: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    HexRange {
        image_id: String,
        path: String,
        offset: u64,
        length: u64,
        #[serde(default)]
        ascii_preview: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    SqliteRow {
        image_id: String,
        path: String,
        table: String,
        #[serde(default)]
        row_id: Option<String>,
        #[serde(default)]
        preview_text: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Custom { detail: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinAnnotation {
    #[serde(default = "default_annotation_kind")]
    pub kind: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub w: f64,
    #[serde(default)]
    pub h: f64,
    #[serde(default)]
    pub arrow_from: Option<[f64; 2]>,
    #[serde(default)]
    pub arrow_to: Option<[f64; 2]>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

fn default_annotation_kind() -> String {
    "box_arrow".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidencePin {
    pub id: String,
    #[serde(default)]
    pub question_id: Option<String>,
    #[serde(default)]
    pub exhibit_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default = "default_pin_source")]
    pub source: String,
    pub target: PinTarget,
    #[serde(default)]
    pub annotation: Option<PinAnnotation>,
    #[serde(default)]
    pub screenshot_rel: Option<String>,
    #[serde(default)]
    pub screenshot_at: Option<String>,
    #[serde(default = "now_iso")]
    pub created_at: String,
}

fn default_pin_source() -> String {
    "desktop".to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub markdown_path: String,
    pub assets_dir: String,
    pub exported_assets: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinAddInput {
    #[serde(default)]
    pub question_id: Option<String>,
    #[serde(default)]
    pub exhibit_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub target: PinTarget,
    #[serde(default)]
    pub annotation: Option<PinAnnotation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionUpsertInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub exhibit_id: Option<String>,
    #[serde(default)]
    pub number: Option<u32>,
    #[serde(default)]
    pub stem: Option<String>,
    #[serde(default)]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub format_hint: Option<String>,
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(default)]
    pub status: Option<QuestionStatus>,
    #[serde(default)]
    pub wp_section_md: Option<String>,
}

/// 生成短 ID（8 位十六进制）。
pub fn new_short_id(prefix: &str) -> String {
    let raw = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}-{}", &raw[..8])
}

pub fn load_workspace(case_dir: &Path) -> anyhow::Result<ContestWorkspace> {
    let path = case_paths::contest_file_path(case_dir);
    if !path.is_file() {
        return Ok(ContestWorkspace::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read contest workspace {}", path.display()))?;
    let ws: ContestWorkspace = serde_json::from_str(&text)
        .with_context(|| format!("parse contest workspace {}", path.display()))?;
    Ok(ws)
}

pub fn save_workspace(case_dir: &Path, ws: &mut ContestWorkspace) -> anyhow::Result<()> {
    let contest_dir = case_paths::contest_dir(case_dir);
    let assets_dir = case_paths::contest_assets_dir(case_dir);
    std::fs::create_dir_all(&assets_dir)
        .with_context(|| format!("create contest assets dir {}", assets_dir.display()))?;
    ws.version = CONTEST_SCHEMA_VERSION;
    ws.updated_at = now_iso();
    let target = case_paths::contest_file_path(case_dir);
    let mut temp = tempfile::NamedTempFile::new_in(&contest_dir)
        .with_context(|| format!("create temp contest file in {}", contest_dir.display()))?;
    let json = serde_json::to_string_pretty(ws)?;
    temp.write_all(json.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(&target)
        .map_err(|err| err.error)
        .with_context(|| format!("persist contest workspace to {}", target.display()))?;
    Ok(())
}

pub fn clear_workspace(case_dir: &Path) -> anyhow::Result<ContestWorkspace> {
    let path = case_paths::contest_file_path(case_dir);
    if path.is_file() {
        std::fs::remove_file(&path)
            .with_context(|| format!("remove contest workspace {}", path.display()))?;
    }
    Ok(ContestWorkspace::default())
}

pub fn save_pin_png(
    case_dir: &Path,
    pin_id: &str,
    png_bytes: &[u8],
) -> anyhow::Result<(String, String)> {
    let safe_id: String = pin_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe_id.is_empty() {
        bail!("invalid pin id");
    }
    let assets_dir = case_paths::contest_assets_dir(case_dir);
    std::fs::create_dir_all(&assets_dir)?;
    let hash_hex = hex::encode(Sha256::digest(png_bytes));
    let file_name = format!("{safe_id}-{}.png", &hash_hex[..8]);
    let target = assets_dir.join(&file_name);
    std::fs::write(&target, png_bytes)
        .with_context(|| format!("write screenshot {}", target.display()))?;
    Ok((format!("assets/{file_name}"), now_iso()))
}

pub fn read_asset_bytes(case_dir: &Path, rel: &str) -> anyhow::Result<Vec<u8>> {
    let path = case_paths::contest_asset_path(case_dir, rel)
        .ok_or_else(|| anyhow::anyhow!("invalid contest asset path: {rel}"))?;
    std::fs::read(&path).with_context(|| format!("read contest asset {}", path.display()))
}

/// 根据检材别名 / 文件名 / SHA256 自动绑定到当前案件已登记的 `EvidenceRef`。
pub fn auto_bind_exhibits(ws: &mut ContestWorkspace, evidence: &[EvidenceRef]) {
    if evidence.is_empty() {
        return;
    }
    let valid_ids: HashSet<&str> = evidence.iter().map(|e| e.image_id.as_str()).collect();
    for exhibit in &mut ws.exhibits {
        if let Some(bound) = exhibit.image_id.as_deref() {
            if !valid_ids.contains(bound) {
                exhibit.image_id = None;
            }
        }
        if exhibit.image_id.is_some() {
            continue;
        }
        let candidates = build_exhibit_match_keys(exhibit);
        let mut best_match: Option<(usize, &EvidenceRef)> = None;
        let mut ambiguous = false;
        for ev in evidence {
            let score = match_score(&candidates, ev);
            if score == 0 {
                continue;
            }
            match best_match {
                None => best_match = Some((score, ev)),
                Some((best_score, _)) if score > best_score => {
                    best_match = Some((score, ev));
                    ambiguous = false;
                }
                Some((best_score, _)) if score == best_score => {
                    ambiguous = true;
                }
                _ => {}
            }
        }
        if !ambiguous {
            if let Some((_, ev)) = best_match {
                exhibit.image_id = Some(ev.image_id.clone());
            }
        }
    }
}

fn build_exhibit_match_keys(exhibit: &ContestExhibit) -> Vec<String> {
    let mut keys = Vec::new();
    for raw in std::iter::once(exhibit.id.as_str())
        .chain(std::iter::once(exhibit.name.as_str()))
        .chain(exhibit.aliases.iter().map(String::as_str))
    {
        let norm = normalize_key(raw);
        if !norm.is_empty() && !keys.contains(&norm) {
            keys.push(norm);
        }
    }
    keys
}

fn normalize_key(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('\\', "/")
}

fn file_stem_lower(path_str: &str) -> (String, String) {
    let normalized = path_str.replace('\\', "/");
    let file_name = normalized
        .rsplit('/')
        .next()
        .unwrap_or(path_str)
        .to_ascii_lowercase();
    let stem = file_name
        .rsplit_once('.')
        .map(|(s, _)| s.to_string())
        .unwrap_or_else(|| file_name.clone());
    (file_name, stem)
}

fn match_score(keys: &[String], ev: &EvidenceRef) -> usize {
    let (file_name, stem) = file_stem_lower(&ev.path);
    let image_id_lower = ev.image_id.to_ascii_lowercase();
    let mut best = 0usize;
    for key in keys {
        if key == &image_id_lower || key == &stem || key == &file_name {
            best = best.max(100);
        } else if !key.is_empty() && (stem.contains(key) || key.contains(&stem)) && stem.len() >= 3
        {
            best = best.max(60);
        } else if !key.is_empty() && file_name.contains(key) {
            best = best.max(40);
        }
    }
    best
}

/// 解析导入内容。`format` 可为 `"auto"` / `"markdown"` / `"md"` / `"json"` / `"exhibits_meta"`。
/// 若 `merge` 为 true，则与已有工作台合并（保留已有作答、笔记、Pin、WP）；否则替换为新赛题集合（保留无题号挂靠的 Pin 可选，默认清空或重映射同名题号）。
pub fn import_content(
    existing: &ContestWorkspace,
    content: &str,
    format: &str,
    title_hint: Option<&str>,
    merge: bool,
) -> anyhow::Result<ContestWorkspace> {
    let trimmed = content.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        bail!("导入内容为空");
    }
    let resolved_format = match format.trim().to_ascii_lowercase().as_str() {
        "json" => "json",
        "exhibits_meta" | "meta" => "exhibits_meta",
        "markdown" | "md" => "markdown",
        "auto" | "" => {
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                "json"
            } else {
                "markdown"
            }
        }
        other => bail!("不支持的导入格式: {other}（支持 auto / markdown / json）"),
    };

    let mut parsed = if resolved_format == "markdown" {
        parse_markdown_contest(trimmed, title_hint)?
    } else {
        parse_json_contest(trimmed, title_hint)?
    };

    if merge {
        Ok(merge_workspaces(existing, parsed))
    } else {
        // 如果之前已经绑定了同名检材的 image_id，尽量保留绑定与 notes/wp
        let old_exhibits: HashMap<String, &ContestExhibit> = existing
            .exhibits
            .iter()
            .map(|e| (e.name.to_ascii_lowercase(), e))
            .collect();
        for ex in &mut parsed.exhibits {
            if let Some(old) = old_exhibits.get(&ex.name.to_ascii_lowercase()) {
                if ex.image_id.is_none() {
                    ex.image_id = old.image_id.clone();
                }
            }
        }
        Ok(parsed)
    }
}

fn merge_workspaces(existing: &ContestWorkspace, incoming: ContestWorkspace) -> ContestWorkspace {
    let mut out = existing.clone();
    if out.title == default_title() && !incoming.title.is_empty() {
        out.title = incoming.title;
    }
    if !incoming.source_format.is_empty() {
        out.source_format = incoming.source_format;
    }
    if !incoming.case_wp_md.trim().is_empty() && out.case_wp_md.trim().is_empty() {
        out.case_wp_md = incoming.case_wp_md;
    }

    for inc_ex in incoming.exhibits {
        if let Some(cur) = out.exhibits.iter_mut().find(|e| {
            e.id.eq_ignore_ascii_case(&inc_ex.id) || e.name.eq_ignore_ascii_case(&inc_ex.name)
        }) {
            if cur.holder.is_none() {
                cur.holder = inc_ex.holder;
            }
            if cur.role.is_none() {
                cur.role = inc_ex.role;
            }
            if cur.device_type.is_none() {
                cur.device_type = inc_ex.device_type;
            }
            if cur.category.is_none() {
                cur.category = inc_ex.category;
            }
            if cur.sha256.is_none() {
                cur.sha256 = inc_ex.sha256;
            }
            if cur.image_id.is_none() {
                cur.image_id = inc_ex.image_id;
            }
            for alias in inc_ex.aliases {
                if !cur.aliases.iter().any(|a| a.eq_ignore_ascii_case(&alias)) {
                    cur.aliases.push(alias);
                }
            }
            if cur.notes_md.trim().is_empty() {
                cur.notes_md = inc_ex.notes_md;
            }
            if cur.wp_md.trim().is_empty() {
                cur.wp_md = inc_ex.wp_md;
            }
        } else {
            out.exhibits.push(inc_ex);
        }
    }

    for inc_q in incoming.questions {
        if let Some(cur) = out.questions.iter_mut().find(|q| q.id == inc_q.id) {
            cur.stem = inc_q.stem;
            if !inc_q.options.is_empty() {
                cur.options = inc_q.options;
            }
            if inc_q.score.is_some() {
                cur.score = inc_q.score;
            }
            if inc_q.format_hint.is_some() {
                cur.format_hint = inc_q.format_hint;
            }
        } else {
            out.questions.push(inc_q);
        }
    }

    for inc_pin in incoming.pins {
        if !out.pins.iter().any(|p| p.id == inc_pin.id) {
            out.pins.push(inc_pin);
        }
    }
    out.updated_at = now_iso();
    out
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawJsonContest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    case_wp_md: Option<String>,
    #[serde(default)]
    exhibits: Vec<RawJsonExhibit>,
    #[serde(default)]
    exhibits_meta: Vec<RawJsonExhibit>,
    #[serde(default)]
    questions: Vec<RawJsonQuestion>,
    #[serde(default)]
    pins: Vec<EvidencePin>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawJsonExhibit {
    #[serde(default, alias = "exhibit_id")]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    holder: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default, alias = "device_type")]
    device_type: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default, alias = "image_id")]
    image_id: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default, alias = "notes_md")]
    notes_md: Option<String>,
    #[serde(default, alias = "wp_md")]
    wp_md: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawJsonQuestion {
    #[serde(default)]
    id: Option<String>,
    #[serde(default, alias = "exhibit_id", alias = "exhibit")]
    exhibit_id: Option<String>,
    #[serde(default)]
    number: Option<u32>,
    stem: String,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default, alias = "format_hint")]
    format_hint: Option<String>,
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    status: Option<QuestionStatus>,
    #[serde(default)]
    notes: Vec<QuestionNote>,
    #[serde(default, alias = "wp_section_md")]
    wp_section_md: Option<String>,
}

/// 解析 JSON 格式赛题或 `exhibits_meta.json`。
pub fn parse_json_contest(
    content: &str,
    title_hint: Option<&str>,
) -> anyhow::Result<ContestWorkspace> {
    let val: serde_json::Value = serde_json::from_str(content).context("无效的 JSON 内容")?;

    // 允许顶层直接是 exhibits_meta 数组
    if let Some(arr) = val.as_array() {
        let exhibits_raw: Vec<RawJsonExhibit> =
            serde_json::from_value(serde_json::Value::Array(arr.clone()))
                .context("无法解析检材元数据数组")?;
        let exhibits = convert_raw_exhibits(exhibits_raw);
        if exhibits.is_empty() {
            bail!("JSON 数组中未找到检材信息");
        }
        return Ok(ContestWorkspace {
            version: CONTEST_SCHEMA_VERSION,
            title: title_hint
                .filter(|t| !t.trim().is_empty())
                .map(|t| t.trim().to_string())
                .unwrap_or_else(default_title),
            source_format: "exhibits_meta".to_string(),
            exhibits,
            ..ContestWorkspace::default()
        });
    }

    let raw: RawJsonContest = serde_json::from_value(val)
        .context("JSON 不符合赛题规范（需含 exhibits / questions 字段）")?;
    let raw_exhibits = if !raw.exhibits.is_empty() {
        raw.exhibits
    } else {
        raw.exhibits_meta
    };
    let mut exhibits = convert_raw_exhibits(raw_exhibits);
    let mut exhibit_ids: HashSet<String> = exhibits.iter().map(|e| e.id.clone()).collect();
    let mut used_qids = HashSet::new();
    let mut questions = Vec::with_capacity(raw.questions.len());

    for (idx, rq) in raw.questions.into_iter().enumerate() {
        let number = rq.number.unwrap_or((idx + 1) as u32);
        let exhibit_id = rq
            .exhibit_id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                exhibits
                    .first()
                    .map(|e| e.id.clone())
                    .unwrap_or_else(|| "EX-DEFAULT".to_string())
            });
        if !exhibit_ids.contains(&exhibit_id) {
            exhibit_ids.insert(exhibit_id.clone());
            exhibits.push(ContestExhibit {
                id: exhibit_id.clone(),
                name: exhibit_id.clone(),
                holder: None,
                role: None,
                device_type: None,
                category: None,
                aliases: vec![exhibit_id.clone()],
                image_id: None,
                sha256: None,
                notes_md: String::new(),
                wp_md: String::new(),
            });
        }
        let base_id = rq
            .id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("Q{:02}", number));
        let qid = ensure_unique_qid(&base_id, &exhibit_id, &mut used_qids);
        let answer = rq.answer.unwrap_or_default();
        let status = rq.status.unwrap_or_else(|| {
            if answer.trim().is_empty() {
                QuestionStatus::Unanswered
            } else {
                QuestionStatus::Answered
            }
        });
        questions.push(ContestQuestion {
            id: qid,
            exhibit_id,
            number,
            stem: rq.stem.trim().to_string(),
            options: rq.options,
            score: rq.score,
            format_hint: rq.format_hint,
            answer,
            status,
            notes: rq.notes,
            wp_section_md: rq.wp_section_md.unwrap_or_default(),
            updated_at: now_iso(),
        });
    }

    if exhibits.is_empty() && questions.is_empty() && raw.pins.is_empty() {
        bail!("JSON 中既无检材也无题目");
    }

    Ok(ContestWorkspace {
        version: CONTEST_SCHEMA_VERSION,
        title: raw
            .title
            .filter(|t| !t.trim().is_empty())
            .or_else(|| {
                title_hint
                    .filter(|t| !t.trim().is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(default_title),
        source_format: "json".to_string(),
        case_wp_md: raw.case_wp_md.unwrap_or_default(),
        exhibits,
        questions,
        pins: raw.pins,
        updated_at: now_iso(),
    })
}

fn convert_raw_exhibits(raw_exhibits: Vec<RawJsonExhibit>) -> Vec<ContestExhibit> {
    let mut exhibits = Vec::with_capacity(raw_exhibits.len());
    for (idx, re) in raw_exhibits.into_iter().enumerate() {
        let name = re
            .name
            .filter(|s| !s.trim().is_empty())
            .or_else(|| re.id.clone())
            .unwrap_or_else(|| format!("检材-{}", idx + 1));
        let id = re
            .id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| slugify_exhibit_id(&name, idx + 1));
        let mut aliases = re.aliases;
        if !aliases.iter().any(|a| a.eq_ignore_ascii_case(&name)) {
            aliases.push(name.clone());
        }
        exhibits.push(ContestExhibit {
            id,
            name,
            holder: re.holder,
            role: re.role,
            device_type: re.device_type,
            category: re.category,
            aliases,
            image_id: re.image_id,
            sha256: re.sha256,
            notes_md: re.notes_md.unwrap_or_default(),
            wp_md: re.wp_md.unwrap_or_default(),
        });
    }
    exhibits
}

/// 解析标准化 Markdown 题库：
/// - `# <标题>`（可选）
/// - `## <检材名>`（每个检材一级分组；检材标题后、首个 `###` 前的文本归为检材案情摘要 `notes_md`）
/// - `### <题号>. <题干>`（也兼容 `### Q01. ...`、`### 1、`、`### 1 ...`）
/// - 自动识别 `（分值:1）` / `（答案格式:...）` 与单行 `A. ` `B. ` 选项
pub fn parse_markdown_contest(
    content: &str,
    title_hint: Option<&str>,
) -> anyhow::Result<ContestWorkspace> {
    let mut title = title_hint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_default();
    let mut exhibits: Vec<ContestExhibit> = Vec::new();
    let mut questions: Vec<ContestQuestion> = Vec::new();
    let mut used_qids: HashSet<String> = HashSet::new();

    let mut cur_exhibit_idx: Option<usize> = None;
    let mut cur_question: Option<ContestQuestion> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();

        // 一级标题作为工作台总标题
        if let Some(h1) = trimmed.strip_prefix("# ") {
            if cur_question.is_none() && title.is_empty() {
                let t = h1.trim();
                if !t.is_empty() {
                    title = t.to_string();
                }
            }
            continue;
        }

        // 二级标题切换检材
        if let Some(h2) = trimmed.strip_prefix("## ") {
            flush_question(&mut cur_question, &mut questions);
            let ex_name = h2.trim();
            if ex_name.is_empty() {
                continue;
            }
            if let Some(idx) = exhibits
                .iter()
                .position(|e| e.name.eq_ignore_ascii_case(ex_name))
            {
                cur_exhibit_idx = Some(idx);
            } else {
                let id = slugify_exhibit_id(ex_name, exhibits.len() + 1);
                exhibits.push(ContestExhibit {
                    id,
                    name: ex_name.to_string(),
                    holder: None,
                    role: None,
                    device_type: None,
                    category: None,
                    aliases: vec![ex_name.to_string()],
                    image_id: None,
                    sha256: None,
                    notes_md: String::new(),
                    wp_md: String::new(),
                });
                cur_exhibit_idx = Some(exhibits.len() - 1);
            }
            continue;
        }

        // 三级标题解析题目
        if let Some(h3) = trimmed.strip_prefix("### ") {
            if let Some((num, rest_stem)) = parse_question_heading(h3.trim()) {
                flush_question(&mut cur_question, &mut questions);
                let ex_idx = match cur_exhibit_idx {
                    Some(idx) => idx,
                    None => {
                        exhibits.push(ContestExhibit {
                            id: "EX-01".to_string(),
                            name: "未分类检材".to_string(),
                            holder: None,
                            role: None,
                            device_type: None,
                            category: None,
                            aliases: Vec::new(),
                            image_id: None,
                            sha256: None,
                            notes_md: String::new(),
                            wp_md: String::new(),
                        });
                        cur_exhibit_idx = Some(0);
                        0
                    }
                };
                let exhibit_id = exhibits[ex_idx].id.clone();
                let (clean_stem, score, format_hint) = extract_stem_annotations(rest_stem);
                let base_id = format!("Q{:02}", num);
                let qid = ensure_unique_qid(&base_id, &exhibit_id, &mut used_qids);
                cur_question = Some(ContestQuestion {
                    id: qid,
                    exhibit_id,
                    number: num,
                    stem: clean_stem,
                    options: Vec::new(),
                    score,
                    format_hint,
                    answer: String::new(),
                    status: QuestionStatus::Unanswered,
                    notes: Vec::new(),
                    wp_section_md: String::new(),
                    updated_at: now_iso(),
                });
                continue;
            }
        }

        // 在当前题目上下文内
        if let Some(q) = cur_question.as_mut() {
            if trimmed.is_empty() || trimmed == "---" {
                continue;
            }
            if is_option_line(trimmed) {
                q.options.push(trimmed.to_string());
            } else if let Some(ans) = strip_answer_prefix(trimmed) {
                if !ans.is_empty() {
                    q.answer = ans.to_string();
                    q.status = QuestionStatus::Answered;
                }
            } else {
                let (line_stem, score, hint) = extract_stem_annotations(trimmed);
                if q.score.is_none() {
                    q.score = score;
                }
                if q.format_hint.is_none() {
                    q.format_hint = hint;
                }
                if !line_stem.is_empty() {
                    if !q.stem.is_empty() {
                        q.stem.push('\n');
                    }
                    q.stem.push_str(&line_stem);
                }
            }
            continue;
        }

        // 在检材上下文但尚未进入题目：归入检材案情摘要 notes_md
        if let Some(ex_idx) = cur_exhibit_idx {
            if !trimmed.is_empty() && trimmed != "---" {
                let notes = &mut exhibits[ex_idx].notes_md;
                if !notes.is_empty() {
                    notes.push('\n');
                }
                notes.push_str(trimmed);
            }
        }
    }

    flush_question(&mut cur_question, &mut questions);

    if questions.is_empty() {
        bail!("Markdown 中未识别到任何题目（请按 `## 检材名` + `### 1. 题干` 格式组织）");
    }

    Ok(ContestWorkspace {
        version: CONTEST_SCHEMA_VERSION,
        title: if title.is_empty() {
            default_title()
        } else {
            title
        },
        source_format: "markdown".to_string(),
        case_wp_md: String::new(),
        exhibits,
        questions,
        pins: Vec::new(),
        updated_at: now_iso(),
    })
}

fn flush_question(slot: &mut Option<ContestQuestion>, out: &mut Vec<ContestQuestion>) {
    if let Some(mut q) = slot.take() {
        q.stem = q.stem.trim().to_string();
        out.push(q);
    }
}

fn slugify_exhibit_id(name: &str, ordinal: usize) -> String {
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if ascii.len() >= 2 && ascii.chars().any(|c| c.is_ascii_alphanumeric()) {
        ascii
    } else {
        format!("EX-{ordinal:02}")
    }
}

fn ensure_unique_qid(base: &str, exhibit_id: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let scoped = format!("{exhibit_id}-{base}");
    if used.insert(scoped.clone()) {
        return scoped;
    }
    let mut seq = 2usize;
    loop {
        let candidate = format!("{scoped}-{seq}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        seq += 1;
    }
}

fn parse_question_heading(heading: &str) -> Option<(u32, &str)> {
    let s = heading
        .strip_prefix('Q')
        .or_else(|| heading.strip_prefix('q'))
        .unwrap_or(heading);
    let digit_len = s.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digit_len == 0 {
        return None;
    }
    let num: u32 = s[..digit_len].parse().ok()?;
    let mut rest = &s[digit_len..];
    for sep in [".", "、", "．", ":", "：", ")", "）"] {
        if let Some(stripped) = rest.strip_prefix(sep) {
            rest = stripped;
            break;
        }
    }
    Some((num, rest.trim()))
}

fn is_option_line(line: &str) -> bool {
    let mut chars = line.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    matches!(chars.next(), Some('.' | '、' | '．' | ')' | '）'))
}

fn strip_answer_prefix(line: &str) -> Option<&str> {
    for prefix in [
        "答案:",
        "答案：",
        "参考答案:",
        "参考答案：",
        "Answer:",
        "answer:",
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(rest.trim());
        }
    }
    None
}

fn extract_stem_annotations(text: &str) -> (String, Option<f64>, Option<String>) {
    let mut working = text.trim().to_string();
    let mut score: Option<f64> = None;
    let mut format_hint: Option<String> = None;

    for (open, close) in [('（', '）'), ('(', ')')] {
        loop {
            let Some(start) = working.find(open) else {
                break;
            };
            let after_open = start + open.len_utf8();
            let Some(rel_end) = working[after_open..].find(close) else {
                break;
            };
            let inner = working[after_open..after_open + rel_end].trim();
            let mut matched = false;
            if let Some(val) = inner
                .strip_prefix("分值:")
                .or_else(|| inner.strip_prefix("分值："))
            {
                if let Ok(parsed) = val.trim().trim_end_matches('分').trim().parse::<f64>() {
                    score = Some(parsed);
                    matched = true;
                }
            } else if let Some(val) = inner
                .strip_prefix("答案格式:")
                .or_else(|| inner.strip_prefix("答案格式："))
            {
                let hint = val.trim();
                if !hint.is_empty() {
                    format_hint = Some(hint.to_string());
                }
                matched = true;
            }
            if matched {
                let end = after_open + rel_end + close.len_utf8();
                working.replace_range(start..end, "");
                working = working.trim().to_string();
            } else {
                break;
            }
        }
    }

    (working, score, format_hint)
}

pub fn resolve_question_index(ws: &ContestWorkspace, key: &str) -> Option<usize> {
    let k = key.trim();
    if let Some(pos) = ws
        .questions
        .iter()
        .position(|q| q.id.eq_ignore_ascii_case(k))
    {
        return Some(pos);
    }
    if let Ok(num) = k.trim_start_matches(['Q', 'q']).parse::<u32>() {
        return ws.questions.iter().position(|q| q.number == num);
    }
    None
}

pub fn resolve_exhibit_index(ws: &ContestWorkspace, key: &str) -> Option<usize> {
    let k = key.trim();
    ws.exhibits.iter().position(|e| {
        e.id.eq_ignore_ascii_case(k)
            || e.name.eq_ignore_ascii_case(k)
            || e.aliases.iter().any(|a| a.eq_ignore_ascii_case(k))
    })
}

pub fn upsert_question(
    ws: &mut ContestWorkspace,
    input: QuestionUpsertInput,
) -> anyhow::Result<ContestQuestion> {
    if let Some(ref id_or_num) = input.id {
        if let Some(idx) = resolve_question_index(ws, id_or_num) {
            let q = &mut ws.questions[idx];
            if let Some(exhibit_id) = input.exhibit_id {
                q.exhibit_id = exhibit_id;
            }
            if let Some(number) = input.number {
                q.number = number;
            }
            if let Some(stem) = input.stem {
                q.stem = stem;
            }
            if let Some(options) = input.options {
                q.options = options;
            }
            if input.score.is_some() {
                q.score = input.score;
            }
            if input.format_hint.is_some() {
                q.format_hint = input.format_hint;
            }
            if let Some(answer) = input.answer {
                q.answer = answer;
                if input.status.is_none()
                    && !q.answer.trim().is_empty()
                    && q.status == QuestionStatus::Unanswered
                {
                    q.status = QuestionStatus::Answered;
                }
            }
            if let Some(status) = input.status {
                q.status = status;
            }
            if let Some(wp) = input.wp_section_md {
                q.wp_section_md = wp;
            }
            q.updated_at = now_iso();
            return Ok(q.clone());
        }
    }

    // 新建题目
    let number = input
        .number
        .unwrap_or_else(|| ws.questions.iter().map(|q| q.number).max().unwrap_or(0) + 1);
    let exhibit_id = input.exhibit_id.unwrap_or_else(|| {
        ws.exhibits
            .first()
            .map(|e| e.id.clone())
            .unwrap_or_else(|| "EX-01".to_string())
    });
    if !ws.exhibits.iter().any(|e| e.id == exhibit_id) {
        ws.exhibits.push(ContestExhibit {
            id: exhibit_id.clone(),
            name: exhibit_id.clone(),
            holder: None,
            role: None,
            device_type: None,
            category: None,
            aliases: vec![exhibit_id.clone()],
            image_id: None,
            sha256: None,
            notes_md: String::new(),
            wp_md: String::new(),
        });
    }
    let mut used: HashSet<String> = ws.questions.iter().map(|q| q.id.clone()).collect();
    let base = input.id.unwrap_or_else(|| format!("Q{number:02}"));
    let id = ensure_unique_qid(&base, &exhibit_id, &mut used);
    let answer = input.answer.unwrap_or_default();
    let status = input.status.unwrap_or_else(|| {
        if answer.trim().is_empty() {
            QuestionStatus::Unanswered
        } else {
            QuestionStatus::Answered
        }
    });
    let created = ContestQuestion {
        id,
        exhibit_id,
        number,
        stem: input.stem.unwrap_or_default(),
        options: input.options.unwrap_or_default(),
        score: input.score,
        format_hint: input.format_hint,
        answer,
        status,
        notes: Vec::new(),
        wp_section_md: input.wp_section_md.unwrap_or_default(),
        updated_at: now_iso(),
    };
    ws.questions.push(created.clone());
    Ok(created)
}

pub fn add_question_note(
    ws: &mut ContestWorkspace,
    question_key: &str,
    author: &str,
    kind: &str,
    command: Option<String>,
    content: String,
) -> anyhow::Result<QuestionNote> {
    let idx = resolve_question_index(ws, question_key)
        .ok_or_else(|| anyhow::anyhow!("未找到题目: {question_key}"))?;
    let note = QuestionNote {
        id: new_short_id("note"),
        author: author.to_string(),
        kind: kind.to_string(),
        command,
        content,
        created_at: now_iso(),
    };
    ws.questions[idx].notes.push(note.clone());
    ws.questions[idx].updated_at = now_iso();
    Ok(note)
}

pub fn add_pin(ws: &mut ContestWorkspace, input: PinAddInput) -> anyhow::Result<EvidencePin> {
    let resolved_qid = match input.question_id {
        Some(ref qk) if !qk.trim().is_empty() => resolve_question_index(ws, qk)
            .map(|i| ws.questions[i].id.clone())
            .or_else(|| Some(qk.trim().to_string())),
        _ => None,
    };
    let resolved_exhibit = match input.exhibit_id {
        Some(ref ek) if !ek.trim().is_empty() => resolve_exhibit_index(ws, ek)
            .map(|i| ws.exhibits[i].id.clone())
            .or_else(|| Some(ek.trim().to_string())),
        _ => resolved_qid.as_ref().and_then(|qid| {
            ws.questions
                .iter()
                .find(|q| &q.id == qid)
                .map(|q| q.exhibit_id.clone())
        }),
    };
    let pin = EvidencePin {
        id: new_short_id("pin"),
        question_id: resolved_qid,
        exhibit_id: resolved_exhibit,
        title: input.title.trim().to_string(),
        summary: input.summary,
        command: input.command.filter(|c| !c.trim().is_empty()),
        source: input.source.unwrap_or_else(default_pin_source),
        target: input.target,
        annotation: input.annotation,
        screenshot_rel: None,
        screenshot_at: None,
        created_at: now_iso(),
    };
    ws.pins.push(pin.clone());
    Ok(pin)
}

/// 生成完整、连续的 Markdown Writeup。
/// 若指定 `exhibit_id` 则仅输出该检材部分；否则输出全局概况 + 各检材 + 独立未关联 Pin。
pub fn render_writeup_markdown(ws: &ContestWorkspace, exhibit_filter: Option<&str>) -> String {
    let mut md = String::new();
    let title = if ws.title.trim().is_empty() {
        default_title()
    } else {
        ws.title.clone()
    };
    md.push_str(&format!("# {title} Writeup\n\n"));

    if exhibit_filter.is_none() && !ws.case_wp_md.trim().is_empty() {
        md.push_str(ws.case_wp_md.trim());
        md.push_str("\n\n");
    }

    let mut rendered_pins: HashSet<String> = HashSet::new();

    for exhibit in &ws.exhibits {
        if let Some(filter) = exhibit_filter {
            if !exhibit.id.eq_ignore_ascii_case(filter)
                && !exhibit.name.eq_ignore_ascii_case(filter)
            {
                continue;
            }
        }
        md.push_str(&format!("## {}\n\n", exhibit.name));

        let mut meta_parts = Vec::new();
        if let Some(h) = exhibit.holder.as_deref().filter(|s| !s.is_empty()) {
            meta_parts.push(format!("持有人: `{h}`"));
        }
        if let Some(r) = exhibit.role.as_deref().filter(|s| !s.is_empty()) {
            meta_parts.push(format!("角色: {r}"));
        }
        if let Some(d) = exhibit.device_type.as_deref().filter(|s| !s.is_empty()) {
            meta_parts.push(format!("设备: `{d}`"));
        }
        if let Some(s) = exhibit.sha256.as_deref().filter(|s| !s.is_empty()) {
            meta_parts.push(format!("SHA256: `{s}`"));
        }
        if !meta_parts.is_empty() {
            md.push_str(&format!("> {}\n\n", meta_parts.join(" | ")));
        }

        if !exhibit.notes_md.trim().is_empty() {
            md.push_str(exhibit.notes_md.trim());
            md.push_str("\n\n");
        }
        if !exhibit.wp_md.trim().is_empty() {
            md.push_str(exhibit.wp_md.trim());
            md.push_str("\n\n");
        }

        let ex_questions: Vec<&ContestQuestion> = ws
            .questions
            .iter()
            .filter(|q| q.exhibit_id == exhibit.id)
            .collect();
        for q in ex_questions {
            md.push_str(&format!("<a id=\"wp-{}\"></a>\n\n", q.id));
            md.push_str(&format!("### {}. {}\n\n", q.number, q.stem));
            if !q.options.is_empty() {
                for opt in &q.options {
                    md.push_str(&format!("- {opt}\n"));
                }
                md.push('\n');
            }
            if !q.answer.trim().is_empty() {
                md.push_str(&format!("**答案：** `{}`\n\n", q.answer.trim()));
            }
            if !q.wp_section_md.trim().is_empty() {
                md.push_str(q.wp_section_md.trim());
                md.push_str("\n\n");
            }
            for note in &q.notes {
                if let Some(cmd) = note.command.as_deref().filter(|c| !c.trim().is_empty()) {
                    md.push_str(&format!("```bash\n{cmd}\n```\n\n"));
                }
                if !note.content.trim().is_empty() {
                    md.push_str(&format!(
                        "- **[{}]** {}\n\n",
                        note.author,
                        note.content.trim()
                    ));
                }
            }
            let q_pins: Vec<&EvidencePin> = ws
                .pins
                .iter()
                .filter(|p| p.question_id.as_deref() == Some(q.id.as_str()))
                .collect();
            for pin in q_pins {
                rendered_pins.insert(pin.id.clone());
                append_pin_markdown(&mut md, pin);
            }
        }

        // 仅关联到检材、未绑定具体题号的 Pin
        let ex_pins: Vec<&EvidencePin> = ws
            .pins
            .iter()
            .filter(|p| {
                !rendered_pins.contains(&p.id)
                    && p.exhibit_id.as_deref() == Some(exhibit.id.as_str())
            })
            .collect();
        if !ex_pins.is_empty() {
            md.push_str("### 检材关键证据固定\n\n");
            for pin in ex_pins {
                rendered_pins.insert(pin.id.clone());
                append_pin_markdown(&mut md, pin);
            }
        }
    }

    // 独立未挂靠检材或题目的 Pin（支持未导入赛题时单独生成图文 WP）
    let orphan_pins: Vec<&EvidencePin> = ws
        .pins
        .iter()
        .filter(|p| !rendered_pins.contains(&p.id))
        .collect();
    if exhibit_filter.is_none() && !orphan_pins.is_empty() {
        md.push_str("## 固定证据与截图记录\n\n");
        for pin in orphan_pins {
            append_pin_markdown(&mut md, pin);
        }
    }

    md
}

fn append_pin_markdown(md: &mut String, pin: &EvidencePin) {
    md.push_str(&format!("#### [证据固定] {}\n\n", pin.title));
    if !pin.summary.trim().is_empty() {
        md.push_str(pin.summary.trim());
        md.push_str("\n\n");
    }
    if let Some(cmd) = pin.command.as_deref().filter(|c| !c.trim().is_empty()) {
        md.push_str(&format!("```bash\n{cmd}\n```\n\n"));
    }
    md.push_str(&format!(
        "- 证据定位: `{}`\n\n",
        describe_target(&pin.target)
    ));
    if let Some(rel) = pin
        .screenshot_rel
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let normalized = if rel.starts_with("./") {
            rel.to_string()
        } else {
            format!("./{rel}")
        };
        md.push_str(&format!("![{}]({normalized})\n\n", pin.title));
    }
}

fn describe_target(target: &PinTarget) -> String {
    match target {
        PinTarget::File {
            image_id,
            path,
            meta_addr,
            ..
        } => {
            if let Some(addr) = meta_addr {
                format!("{image_id}:{path} (meta: {addr})")
            } else {
                format!("{image_id}:{path}")
            }
        }
        PinTarget::AnalysisRecord {
            image_id,
            category_id,
            category_name,
            record_id,
            ..
        } => {
            let cat = category_name.as_deref().unwrap_or(category_id);
            format!("{image_id} / {cat} #{record_id}")
        }
        PinTarget::HexRange {
            image_id,
            path,
            offset,
            length,
            ..
        } => format!("{image_id}:{path} @ 0x{offset:X}+0x{length:X}"),
        PinTarget::SqliteRow {
            image_id,
            path,
            table,
            row_id,
            ..
        } => {
            if let Some(rid) = row_id {
                format!("{image_id}:{path} [{table}#{rid}]")
            } else {
                format!("{image_id}:{path} [{table}]")
            }
        }
        PinTarget::Custom { detail } => detail.clone(),
    }
}

/// 导出 Markdown Writeup 到 `output_md_path`，并把引用的 PNG 截图复制到同级 `assets/` 目录。
pub fn export_writeup(
    case_dir: &Path,
    ws: &ContestWorkspace,
    output_md_path: &Path,
    exhibit_filter: Option<&str>,
) -> anyhow::Result<ExportResult> {
    let parent = output_md_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create export parent dir {}", parent.display()))?;
    let target_assets_dir = parent.join(case_paths::CONTEST_ASSETS_DIR);
    std::fs::create_dir_all(&target_assets_dir)
        .with_context(|| format!("create export assets dir {}", target_assets_dir.display()))?;

    let markdown = render_writeup_markdown(ws, exhibit_filter);
    let mut copied = 0usize;
    let mut seen_assets = HashSet::new();
    for pin in &ws.pins {
        if let Some(rel) = pin.screenshot_rel.as_deref() {
            if !seen_assets.insert(rel.to_string()) {
                continue;
            }
            if let Some(src) = case_paths::contest_asset_path(case_dir, rel) {
                if src.is_file() {
                    if let Some(name) = src.file_name() {
                        let dst = target_assets_dir.join(name);
                        if src != dst {
                            std::fs::copy(&src, &dst).with_context(|| {
                                format!("copy asset {} to {}", src.display(), dst.display())
                            })?;
                        }
                        copied += 1;
                    }
                }
            }
        }
    }

    std::fs::write(output_md_path, markdown.as_bytes())
        .with_context(|| format!("write writeup {}", output_md_path.display()))?;

    Ok(ExportResult {
        markdown_path: output_md_path.to_string_lossy().to_string(),
        assets_dir: target_assets_dir.to_string_lossy().to_string(),
        exported_assets: copied,
    })
}

pub fn contest_markdown_spec_doc() -> &'static str {
    "# LovelyCarve 赛题 Markdown 导入规范

- `# <比赛/案件总标题>`（可选）
- `## <检材名称>`（按检材分组；紧随其后的普通文本作为检材案情摘要）
- `### <题号>. <题干>`（可在括号中附带 `（分值:1）` 与 `（答案格式:...）`）
- 单行选项使用 `A. `、`B. `、`C. `、`D. ` 开头
- `答案: <内容>`（可选，预填参考或已解答案）
"
}

pub fn workspace_summary_json(ws: &ContestWorkspace) -> serde_json::Value {
    let total = ws.questions.len();
    let answered = ws
        .questions
        .iter()
        .filter(|q| q.status == QuestionStatus::Answered)
        .count();
    let bound = ws.exhibits.iter().filter(|e| e.image_id.is_some()).count();
    let captured = ws
        .pins
        .iter()
        .filter(|p| p.screenshot_rel.is_some())
        .count();
    serde_json::json!({
        "title": ws.title,
        "exhibits": ws.exhibits.len(),
        "boundExhibits": bound,
        "questions": total,
        "answered": answered,
        "pins": ws.pins.len(),
        "capturedPins": captured,
        "updatedAt": ws.updated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_markdown_question_bank_preserves_exhibits_scores_and_options() {
        let md = r#"# 美亚杯个人赛

## VTM-1

张三随身携带的笔记本电脑镜像。

### 1. 请问该检材系统安装的操作系统版本号是？（分值:1）（答案格式:10.0.xxxxx）
A. 10.0.19045
B. 10.0.22631

### 2. 用户登录的最后时间（UTC）？
答案: 2025-11-01 08:00:00

## VTM-2

### 1. 手机通讯录中共有多少条记录？（分值:2）
"#;
        let ws = parse_markdown_contest(md, None).expect("parse markdown");
        assert_eq!(ws.title, "美亚杯个人赛");
        assert_eq!(ws.exhibits.len(), 2);
        assert_eq!(ws.exhibits[0].name, "VTM-1");
        assert!(ws.exhibits[0].notes_md.contains("笔记本电脑"));
        assert_eq!(ws.questions.len(), 3);
        assert_eq!(ws.questions[0].id, "Q01");
        assert_eq!(ws.questions[0].score, Some(1.0));
        assert_eq!(ws.questions[0].format_hint.as_deref(), Some("10.0.xxxxx"));
        assert_eq!(ws.questions[0].options.len(), 2);
        assert_eq!(ws.questions[1].status, QuestionStatus::Answered);
        assert_eq!(ws.questions[1].answer, "2025-11-01 08:00:00");
        // 第二个检材重复 ### 1. 时自动消歧 id，保留题号 1
        assert_eq!(ws.questions[2].number, 1);
        assert_ne!(ws.questions[2].id, "Q01");
    }

    #[test]
    fn auto_bind_matches_exhibit_aliases_and_export_copies_assets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let case_dir = dir.path().join("case");
        std::fs::create_dir_all(&case_dir).expect("mkdir");

        let meta = r#"[
          { "exhibit_id": "VTM-1", "holder": "张三", "aliases": ["suspect-pc"] }
        ]"#;
        let mut ws = parse_json_contest(meta, Some("团队赛")).expect("meta");
        let ev = vec![EvidenceRef {
            image_id: "img-vtm1".to_string(),
            path: "D:/Images/suspect-pc.E01".to_string(),
            source_format: crate::core::evidence::image::ImageFormat::Ewf,
            size_bytes: 1024,
            added_at: now_iso(),
            platform: None,
            manual_platform: None,
            evidence_type: None,
        }];
        auto_bind_exhibits(&mut ws, &ev);
        assert_eq!(ws.exhibits[0].image_id.as_deref(), Some("img-vtm1"));

        let mut pin = add_pin(
            &mut ws,
            PinAddInput {
                question_id: None,
                exhibit_id: Some("VTM-1".to_string()),
                title: "关键注册表项".to_string(),
                summary: "证明开启了自动登录".to_string(),
                command: Some("lovelycarve-cli ls /suspect-pc/Windows".to_string()),
                source: Some("cli".to_string()),
                target: PinTarget::File {
                    image_id: "img-vtm1".to_string(),
                    volume_key: None,
                    node_id: Some(42),
                    path: "/Windows/System32/config/SAM".to_string(),
                    meta_addr: None,
                },
                annotation: None,
            },
        )
        .expect("add pin");
        let (rel, ts) = save_pin_png(&case_dir, &pin.id, b"\x89PNG\r\n\x1a\nTEST").expect("png");
        pin.screenshot_rel = Some(rel.clone());
        pin.screenshot_at = Some(ts);
        ws.pins[0] = pin;
        save_workspace(&case_dir, &mut ws).expect("save");

        let export_md = dir.path().join("out").join("wp.md");
        let res = export_writeup(&case_dir, &ws, &export_md, None).expect("export");
        assert_eq!(res.exported_assets, 1);
        let text = std::fs::read_to_string(&export_md).expect("read wp");
        assert!(text.contains("./assets/pin-"));
        assert!(PathBuf::from(&res.assets_dir).exists());
    }
}
