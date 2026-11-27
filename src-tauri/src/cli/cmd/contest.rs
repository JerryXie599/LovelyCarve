//! `contest` / `pin` / `wp`: CLI 赛题管理、证据标注与 Writeup 协同命令。
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cli::{
    browsing::LastLink,
    exec::Exec,
    render,
    types::{parse_address, parse_host_path, HostPath},
    CliError, Output,
};
use crate::core::casework::contest::{
    self, PinAddInput, PinTarget, QuestionStatus, QuestionUpsertInput,
};
use crate::host::State;

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContestArgs {
    #[command(subcommand)]
    pub command: Option<ContestCommand>,
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContestCommand {
    /// 查看赛题与检材映射概况（默认）
    Ls,
    /// 查看导入规范与示例提示
    Spec,
    /// 导入 Markdown / JSON 赛题或 exhibits_meta.json
    Import(ContestImportArgs),
    /// 查看单道题干、选项、作答与笔记
    Show(QuestionIdArg),
    /// 绑定竞赛检材到案件证据（imageId 或别名）
    Bind(ExhibitBindArgs),
    /// 填写题目答案与状态（可同时附加笔记或 CLI 指令）
    Answer(QuestionAnswerArgs),
}

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContestImportArgs {
    /// 赛题文件路径（.md 或 .json）
    #[arg(value_parser = parse_host_path, value_name = "文件")]
    pub file: HostPath,
    /// 格式：auto / markdown / json / exhibits_meta
    #[arg(long, default_value = "auto")]
    pub format: String,
    /// 工作台标题覆盖
    #[arg(long)]
    pub title: Option<String>,
    /// 合并到现有题库（保留已填答案、Pin 与笔记）
    #[arg(long)]
    pub merge: bool,
}

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionIdArg {
    /// 题号或 ID（如 Q01 或 1）
    #[arg(value_name = "题号")]
    pub question: String,
}

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExhibitBindArgs {
    /// 竞赛检材名称或 ID（如 VTM-1）
    #[arg(value_name = "竞赛检材")]
    pub exhibit: String,
    /// 案件检材别名或 imageId
    #[arg(value_name = "案件检材")]
    pub evidence: String,
}

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionAnswerArgs {
    /// 题号或 ID（如 Q01 或 1）
    #[arg(value_name = "题号")]
    pub question: String,
    /// 答案内容
    #[arg(value_name = "答案")]
    pub answer: String,
    /// 题目状态：answered / review / stuck / unanswered
    #[arg(long)]
    pub status: Option<String>,
    /// 顺带附加一条做题笔记
    #[arg(long)]
    pub note: Option<String>,
    /// 顺带记录本次取证命令行
    #[arg(long)]
    pub cmd: Option<String>,
}

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinArgs {
    #[command(subcommand)]
    pub command: Option<PinCommand>,
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PinCommand {
    /// 列出已固定的证据 Pin（默认）
    Ls(PinLsArgs),
    /// 新增证据固定 Pin（支持文件地址、分析记录或十六进制区间）
    Add(PinAddArgs),
    /// 删除证据 Pin
    Rm(PinIdArg),
}

#[derive(Args, Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinLsArgs {
    /// 仅查看某题关联的 Pin
    #[arg(long)]
    pub question: Option<String>,
}

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinAddArgs {
    /// 检材文件地址（如 /nas/part:0/etc/passwd）或自定义证据描述
    #[arg(value_name = "目标")]
    pub target: Option<String>,
    /// Pin 标题
    #[arg(long, value_name = "标题")]
    pub title: String,
    /// 关联题号（可选，未导入题目也可先留证）
    #[arg(long, value_name = "题号")]
    pub question: Option<String>,
    /// 关联竞赛检材（可选）
    #[arg(short = 'e', long, value_name = "检材")]
    pub exhibit: Option<String>,
    /// 证据说明 / 取证结论摘要
    #[arg(long, default_value = "")]
    pub summary: String,
    /// 记录使用的取证命令行
    #[arg(long)]
    pub cmd: Option<String>,
    /// 若固定插件分析记录，指定记录 ID
    #[arg(long)]
    pub record: Option<i64>,
    /// 若固定插件分析记录，指定分类 ID
    #[arg(long)]
    pub category: Option<String>,
    /// 十六进制偏移（字节）
    #[arg(long)]
    pub offset: Option<u64>,
    /// 十六进制长度（字节）
    #[arg(long, default_value_t = 64)]
    pub length: u64,
}

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinIdArg {
    /// Pin ID
    #[arg(value_name = "PIN_ID")]
    pub pin_id: String,
}

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WpArgs {
    #[command(subcommand)]
    pub command: Option<WpCommand>,
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WpCommand {
    /// 预览生成的 Markdown Writeup（默认）
    Show(WpScopeArgs),
    /// 为题目追加一条命令行或取证笔记
    Note(WpNoteArgs),
    /// 导出 Markdown + `./assets/*.png` 截图文件夹
    Export(WpExportArgs),
}

#[derive(Args, Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WpScopeArgs {
    /// 仅预览指定检材
    #[arg(short = 'e', long)]
    pub exhibit: Option<String>,
}

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WpNoteArgs {
    /// 题号或 ID（如 Q01 或 1）
    #[arg(value_name = "题号")]
    pub question: String,
    /// 笔记内容
    #[arg(value_name = "内容")]
    pub content: String,
    /// 附带命令行记录
    #[arg(long)]
    pub cmd: Option<String>,
}

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WpExportArgs {
    /// 导出 Markdown 文件路径
    #[arg(value_parser = parse_host_path, value_name = "输出MD")]
    pub output: HostPath,
    /// 仅导出指定检材
    #[arg(short = 'e', long)]
    pub exhibit: Option<String>,
}

pub fn contest_path(args: &ContestArgs) -> &'static str {
    match &args.command {
        None | Some(ContestCommand::Ls) => "contest ls",
        Some(ContestCommand::Spec) => "contest spec",
        Some(ContestCommand::Import(_)) => "contest import",
        Some(ContestCommand::Show(_)) => "contest show",
        Some(ContestCommand::Bind(_)) => "contest bind",
        Some(ContestCommand::Answer(_)) => "contest answer",
    }
}

pub fn pin_path(args: &PinArgs) -> &'static str {
    match &args.command {
        None | Some(PinCommand::Ls(_)) => "pin ls",
        Some(PinCommand::Add(_)) => "pin add",
        Some(PinCommand::Rm(_)) => "pin rm",
    }
}

pub fn wp_path(args: &WpArgs) -> &'static str {
    match &args.command {
        None | Some(WpCommand::Show(_)) => "wp show",
        Some(WpCommand::Note(_)) => "wp note",
        Some(WpCommand::Export(_)) => "wp export",
    }
}

pub async fn run_contest(exec: &Exec, args: ContestArgs) -> Result<Output, CliError> {
    match args.command.unwrap_or(ContestCommand::Ls) {
        ContestCommand::Spec => Ok(Output::json(json!({
            "spec": contest::contest_markdown_spec_doc()
        }))),
        ContestCommand::Ls => {
            let ws = crate::commands::casework::contest::contest_get(State::new(&exec.state))
                .await
                .map_err(|e| crate::cli::command_error(e).in_operation("contest_get"))?;
            Ok(Output::json(json!({
                "summary": contest::workspace_summary_json(&ws),
                "exhibits": ws.exhibits,
                "questions": ws.questions,
                "pins": ws.pins,
            })))
        }
        ContestCommand::Import(args) => {
            let content = std::fs::read_to_string(args.file.as_path()).map_err(|err| {
                CliError::new(
                    "io",
                    format!("读取赛题文件失败 {}: {err}", args.file.text()),
                )
            })?;
            let ws = crate::commands::casework::contest::contest_import(
                exec.host.clone(),
                State::new(&exec.state),
                content,
                Some(args.format),
                args.title,
                Some(args.merge),
            )
            .await
            .map_err(|e| crate::cli::command_error(e).in_operation("contest_import"))?;
            Ok(Output::json(contest::workspace_summary_json(&ws)))
        }
        ContestCommand::Show(args) => {
            let ws = crate::commands::casework::contest::contest_get(State::new(&exec.state))
                .await
                .map_err(|e| crate::cli::command_error(e).in_operation("contest_get"))?;
            let idx = contest::resolve_question_index(&ws, &args.question).ok_or_else(|| {
                CliError::new("notFound", format!("未找到题目: {}", args.question))
            })?;
            let q = &ws.questions[idx];
            let pins: Vec<_> = ws
                .pins
                .iter()
                .filter(|p| p.question_id.as_deref() == Some(q.id.as_str()))
                .collect();
            Ok(Output::json(json!({
                "question": q,
                "pins": pins,
            })))
        }
        ContestCommand::Bind(args) => {
            let evidence_items = exec.evidence().await?;
            let resolved_id = evidence_items
                .iter()
                .find(|ev| {
                    ev.image_id.as_str() == args.evidence
                        || ev.alias.eq_ignore_ascii_case(&args.evidence)
                })
                .map(|ev| ev.image_id.to_string())
                .ok_or_else(|| {
                    CliError::new("notFound", format!("案件中未找到检材: {}", args.evidence))
                })?;
            let ex = crate::commands::casework::contest::contest_exhibit_bind(
                exec.host.clone(),
                State::new(&exec.state),
                args.exhibit,
                Some(resolved_id),
            )
            .await
            .map_err(|e| crate::cli::command_error(e).in_operation("contest_exhibit_bind"))?;
            Ok(Output::json(serde_json::to_value(ex)?))
        }
        ContestCommand::Answer(args) => {
            let status = args
                .status
                .as_deref()
                .map(|raw| {
                    QuestionStatus::parse(raw).ok_or_else(|| {
                        CliError::invalid(format!(
                            "无效状态 {raw}，可用: answered / review / stuck / unanswered"
                        ))
                    })
                })
                .transpose()?;
            let q = crate::commands::casework::contest::contest_question_upsert(
                exec.host.clone(),
                State::new(&exec.state),
                QuestionUpsertInput {
                    id: Some(args.question.clone()),
                    exhibit_id: None,
                    number: None,
                    stem: None,
                    options: None,
                    score: None,
                    format_hint: None,
                    answer: Some(args.answer),
                    status,
                    wp_section_md: None,
                },
            )
            .await
            .map_err(|e| crate::cli::command_error(e).in_operation("contest_question_upsert"))?;
            if args.note.is_some() || args.cmd.is_some() {
                crate::commands::casework::contest::contest_question_note_add(
                    exec.host.clone(),
                    State::new(&exec.state),
                    q.id.clone(),
                    Some("cli".to_string()),
                    Some(
                        if args.cmd.is_some() {
                            "command"
                        } else {
                            "note"
                        }
                        .to_string(),
                    ),
                    args.cmd,
                    args.note.unwrap_or_default(),
                )
                .await
                .map_err(|e| {
                    crate::cli::command_error(e).in_operation("contest_question_note_add")
                })?;
            }
            Ok(Output::json(serde_json::to_value(q)?))
        }
    }
}

pub async fn run_pin(exec: &Exec, args: PinArgs) -> Result<Output, CliError> {
    match args.command.unwrap_or(PinCommand::Ls(PinLsArgs::default())) {
        PinCommand::Ls(args) => {
            let ws = crate::commands::casework::contest::contest_get(State::new(&exec.state))
                .await
                .map_err(|e| crate::cli::command_error(e).in_operation("contest_get"))?;
            let items: Vec<_> = match args.question {
                Some(ref qk) => {
                    let qid = contest::resolve_question_index(&ws, qk)
                        .map(|i| ws.questions[i].id.clone())
                        .unwrap_or_else(|| qk.clone());
                    ws.pins
                        .into_iter()
                        .filter(|p| p.question_id.as_deref() == Some(qid.as_str()))
                        .collect()
                }
                None => ws.pins,
            };
            Ok(Output::json(json!({ "pins": items })))
        }
        PinCommand::Add(args) => {
            let target = resolve_pin_target(exec, &args).await?;
            let pin = crate::commands::casework::contest::contest_pin_add(
                exec.host.clone(),
                State::new(&exec.state),
                PinAddInput {
                    question_id: args.question,
                    exhibit_id: args.exhibit,
                    title: args.title,
                    summary: args.summary,
                    command: args.cmd,
                    source: Some("cli".to_string()),
                    target,
                    annotation: None,
                },
            )
            .await
            .map_err(|e| crate::cli::command_error(e).in_operation("contest_pin_add"))?;
            Ok(Output::json(serde_json::to_value(pin)?))
        }
        PinCommand::Rm(args) => {
            let ws = crate::commands::casework::contest::contest_pin_remove(
                exec.host.clone(),
                State::new(&exec.state),
                args.pin_id.clone(),
            )
            .await
            .map_err(|e| crate::cli::command_error(e).in_operation("contest_pin_remove"))?;
            Ok(Output::json(json!({
                "removed": args.pin_id,
                "remainingPins": ws.pins.len(),
            })))
        }
    }
}

async fn resolve_pin_target(exec: &Exec, args: &PinAddArgs) -> Result<PinTarget, CliError> {
    if let Some(record_id) = args.record {
        let image_id = match args.target.as_deref() {
            Some(alias) if !alias.trim().is_empty() => exec
                .evidence()
                .await?
                .into_iter()
                .find(|ev| {
                    ev.image_id.as_str() == alias
                        || ev.alias.eq_ignore_ascii_case(alias.trim_start_matches('/'))
                })
                .map(|ev| ev.image_id.to_string())
                .unwrap_or_else(|| alias.to_string()),
            _ => exec
                .evidence()
                .await?
                .into_iter()
                .next()
                .map(|ev| ev.image_id.to_string())
                .unwrap_or_default(),
        };
        return Ok(PinTarget::AnalysisRecord {
            image_id,
            category_id: args
                .category
                .clone()
                .unwrap_or_else(|| "analysis".to_string()),
            category_name: None,
            record_id,
            preview_text: (!args.summary.is_empty()).then(|| args.summary.clone()),
        });
    }

    if let Some(ref raw_target) = args.target {
        if raw_target.starts_with('/') {
            if let Ok(addr) = parse_address(raw_target) {
                if let Ok(resolved) = exec.target(&addr, LastLink::Follow).await {
                    if let Some(offset) = args.offset {
                        return Ok(PinTarget::HexRange {
                            image_id: resolved.image_id().to_string(),
                            path: resolved.address(),
                            offset,
                            length: args.length,
                            ascii_preview: None,
                        });
                    }
                    let node_id = resolved.node_id().parse::<i64>().ok();
                    return Ok(PinTarget::File {
                        image_id: resolved.image_id().to_string(),
                        volume_key: resolved.location.first().map(|(_, id)| id.clone()),
                        node_id,
                        path: resolved.address(),
                        meta_addr: (!resolved.node_id().is_empty())
                            .then(|| resolved.node_id().to_string()),
                    });
                }
            }
        }
        return Ok(PinTarget::Custom {
            detail: raw_target.clone(),
        });
    }

    Ok(PinTarget::Custom {
        detail: args.title.clone(),
    })
}

pub async fn run_wp(exec: &Exec, args: WpArgs) -> Result<Output, CliError> {
    match args
        .command
        .unwrap_or(WpCommand::Show(WpScopeArgs::default()))
    {
        WpCommand::Show(args) => {
            let markdown = crate::commands::casework::contest::contest_wp_preview(
                State::new(&exec.state),
                args.exhibit,
            )
            .await
            .map_err(|e| crate::cli::command_error(e).in_operation("contest_wp_preview"))?;
            Ok(Output::json(json!({ "markdown": markdown })))
        }
        WpCommand::Note(args) => {
            let note = crate::commands::casework::contest::contest_question_note_add(
                exec.host.clone(),
                State::new(&exec.state),
                args.question,
                Some("cli".to_string()),
                Some(
                    if args.cmd.is_some() {
                        "command"
                    } else {
                        "note"
                    }
                    .to_string(),
                ),
                args.cmd,
                args.content,
            )
            .await
            .map_err(|e| crate::cli::command_error(e).in_operation("contest_question_note_add"))?;
            Ok(Output::json(serde_json::to_value(note)?))
        }
        WpCommand::Export(args) => {
            let res = crate::commands::casework::contest::contest_wp_export(
                State::new(&exec.state),
                args.output.text(),
                args.exhibit,
            )
            .await
            .map_err(|e| crate::cli::command_error(e).in_operation("contest_wp_export"))?;
            Ok(Output::json(serde_json::to_value(res)?))
        }
    }
}

pub fn render_contest(args: &ContestArgs, value: &Value) -> Option<String> {
    match &args.command {
        Some(ContestCommand::Spec) => value["spec"].as_str().map(str::to_owned),
        None | Some(ContestCommand::Ls) => {
            let items = value["questions"].as_array().map_or(&[][..], Vec::as_slice);
            let mut out = format!(
                "工作台: {} | 题目: {}/{} 已答 | 检材: {}/{} 已映射 | Pin: {}\n\n",
                value["summary"]["title"].as_str().unwrap_or("-"),
                value["summary"]["answered"],
                value["summary"]["questions"],
                value["summary"]["boundExhibits"],
                value["summary"]["exhibits"],
                value["summary"]["pins"],
            );
            if !items.is_empty() {
                out.push_str(&render::table(
                    items,
                    &[
                        ("ID", "id"),
                        ("检材", "exhibitId"),
                        ("状态", "status"),
                        ("答案", "answer"),
                        ("题干", "stem"),
                    ],
                ));
            }
            Some(out)
        }
        _ => None,
    }
}

pub fn render_pin(args: &PinArgs, value: &Value) -> Option<String> {
    match &args.command {
        None | Some(PinCommand::Ls(_)) => {
            let items = value["pins"].as_array().map_or(&[][..], Vec::as_slice);
            if items.is_empty() {
                Some("（暂无证据 Pin）\n".to_owned())
            } else {
                Some(render::table(
                    items,
                    &[
                        ("ID", "id"),
                        ("题号", "questionId"),
                        ("来源", "source"),
                        ("标题", "title"),
                        ("截图", "screenshotRel"),
                    ],
                ))
            }
        }
        _ => None,
    }
}

pub fn render_wp(args: &WpArgs, value: &Value) -> Option<String> {
    match &args.command {
        None | Some(WpCommand::Show(_)) => value["markdown"].as_str().map(str::to_owned),
        _ => None,
    }
}
