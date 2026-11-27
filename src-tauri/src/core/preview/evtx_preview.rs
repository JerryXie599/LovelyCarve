//! EVTX Windows Event Log native preview for files inside a forensic image.
//!
//! Provides file-level digest (channel, chunk counts, event ID distributions,
//! time ranges) and fast in-memory paged record viewing with decoded titles,
//! level classifications, event data parameters, and full XML fidelity.

use std::collections::HashMap;
use std::io::{Cursor, Seek, SeekFrom};
use std::sync::Arc;

use byteorder::{LittleEndian, ReadBytesExt};
use chrono::DateTime;
use evtx::EvtxParser;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;
use serde::{Deserialize, Serialize};

pub const EVTX_MAGIC: &[u8] = b"ElfFile\x00";
pub const MAX_EVTX_RECORDS: usize = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvtxDigest {
    pub file_size: u64,
    pub chunk_count: u64,
    pub next_record_id: u64,
    pub is_dirty: bool,
    pub is_full: bool,
    pub major_version: u16,
    pub minor_version: u16,
    pub total_records: usize,
    pub channel: Option<String>,
    pub computer: Option<String>,
    pub earliest_time: Option<String>,
    pub latest_time: Option<String>,
    pub event_id_distribution: Vec<EventCount>,
    pub level_distribution: Vec<LevelCount>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventCount {
    pub event_id: u32,
    pub count: usize,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelCount {
    pub level: u8,
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvtxPage {
    pub total: usize,
    pub filtered_total: usize,
    pub offset: usize,
    pub limit: usize,
    pub records: Vec<EvtxRecordItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvtxRecordItem {
    pub record_id: u64,
    pub timestamp: String,
    pub event_id: u32,
    pub level: u8,
    pub level_name: String,
    pub provider: String,
    pub channel: String,
    pub computer: String,
    pub title: String,
    pub summary: String,
    pub event_data: Vec<EventDataField>,
    pub xml: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDataField {
    pub name: String,
    pub value: String,
}

/// Parses an in-memory buffer of an EVTX file into its digest and record list.
pub fn parse_evtx(
    bytes: Arc<[u8]>,
    file_size: u64,
) -> Result<(EvtxDigest, Vec<EvtxRecordItem>), String> {
    if bytes.len() < 128 || &bytes[..8] != EVTX_MAGIC {
        return Err("文件缺少 ElfFile 签名，不是有效的 Windows EVTX 日志文件".to_string());
    }

    let mut cursor = Cursor::new(&bytes[..]);
    cursor.seek(SeekFrom::Start(8)).map_err(|e| e.to_string())?;

    let _oldest_chunk = cursor.read_u64::<LittleEndian>().unwrap_or(0);
    let _current_chunk_num = cursor.read_u64::<LittleEndian>().unwrap_or(0);
    let next_record_num = cursor.read_u64::<LittleEndian>().unwrap_or(0);
    let _header_size = cursor.read_u32::<LittleEndian>().unwrap_or(4096);
    let minor_version = cursor.read_u16::<LittleEndian>().unwrap_or(1);
    let major_version = cursor.read_u16::<LittleEndian>().unwrap_or(3);
    let _header_block_size = cursor.read_u16::<LittleEndian>().unwrap_or(4096);
    let chunk_count_hdr = cursor.read_u16::<LittleEndian>().unwrap_or(0);

    cursor
        .seek(SeekFrom::Start(120))
        .map_err(|e| e.to_string())?;
    let flags = cursor.read_u32::<LittleEndian>().unwrap_or(0);
    let is_dirty = (flags & 0x1) != 0;
    let is_full = (flags & 0x2) != 0;

    let calculated_chunks = if bytes.len() > 4096 {
        ((bytes.len() - 4096) / 65536) as u64
    } else {
        chunk_count_hdr as u64
    };

    let cursor = Cursor::new(bytes);
    let mut parser =
        EvtxParser::from_read_seek(cursor).map_err(|e| format!("EVTX 解析器初始化失败: {e}"))?;

    let mut records: Vec<EvtxRecordItem> = Vec::new();
    let mut event_counts: HashMap<u32, usize> = HashMap::new();
    let mut level_counts: HashMap<u8, usize> = HashMap::new();
    let mut primary_channel: Option<String> = None;
    let mut primary_computer: Option<String> = None;
    let mut earliest_time: Option<String> = None;
    let mut latest_time: Option<String> = None;
    let mut notes: Vec<String> = Vec::new();

    for res in parser.records() {
        let record = match res {
            Ok(r) => r,
            Err(_) => continue,
        };

        let xml = record.data;
        let event_id = extract_event_id(&xml).unwrap_or(0);
        let row = extract_row(&xml, record.event_record_id, &record.timestamp);
        let event_data = parse_event_data(&xml);
        let summary = build_summary(event_id, &row.provider, &event_data);
        let title = event_title(&row.provider, event_id);
        let level_name = level_to_name(row.level);

        *event_counts.entry(event_id).or_insert(0) += 1;
        *level_counts.entry(row.level).or_insert(0) += 1;

        if primary_channel.is_none() && !row.channel.is_empty() {
            primary_channel = Some(row.channel.clone());
        }
        if primary_computer.is_none() && !row.computer.is_empty() {
            primary_computer = Some(row.computer.clone());
        }

        if earliest_time.is_none()
            || earliest_time.as_deref().unwrap_or("") > row.timestamp.as_str()
        {
            earliest_time = Some(row.timestamp.clone());
        }
        if latest_time.is_none() || latest_time.as_deref().unwrap_or("") < row.timestamp.as_str() {
            latest_time = Some(row.timestamp.clone());
        }

        records.push(EvtxRecordItem {
            record_id: row.record_id,
            timestamp: row.timestamp,
            event_id,
            level: row.level,
            level_name,
            provider: row.provider,
            channel: row.channel,
            computer: row.computer,
            title: title.to_string(),
            summary,
            event_data,
            xml,
        });

        if records.len() >= MAX_EVTX_RECORDS {
            notes.push(format!(
                "日志条目过多，已加载前 {MAX_EVTX_RECORDS} 条记录作预览展示。"
            ));
            break;
        }
    }

    if is_dirty {
        notes.push(
            "日志文件标记为未完全关闭 (Dirty 状态)，可能包含未刷新或损坏的尾部区块。".to_string(),
        );
    }

    let mut event_id_distribution: Vec<EventCount> = event_counts
        .into_iter()
        .map(|(id, count)| EventCount {
            event_id: id,
            count,
            title: event_title("", id).to_string(),
        })
        .collect();
    event_id_distribution.sort_by(|a, b| b.count.cmp(&a.count));
    if event_id_distribution.len() > 30 {
        event_id_distribution.truncate(30);
    }

    let mut level_distribution: Vec<LevelCount> = level_counts
        .into_iter()
        .map(|(lvl, count)| LevelCount {
            level: lvl,
            name: level_to_name(lvl),
            count,
        })
        .collect();
    level_distribution.sort_by_key(|l| l.level);

    let digest = EvtxDigest {
        file_size,
        chunk_count: calculated_chunks,
        next_record_id: next_record_num,
        is_dirty,
        is_full,
        major_version,
        minor_version,
        total_records: records.len(),
        channel: primary_channel,
        computer: primary_computer,
        earliest_time,
        latest_time,
        event_id_distribution,
        level_distribution,
        notes,
    };

    Ok((digest, records))
}

/// Slices and filters records according to query, level, and event_id.
pub fn filter_records(
    records: &[EvtxRecordItem],
    query: Option<&str>,
    level: Option<u8>,
    event_id: Option<u32>,
    offset: usize,
    limit: usize,
) -> EvtxPage {
    let total = records.len();
    let q = query
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());

    let filtered: Vec<&EvtxRecordItem> = records
        .iter()
        .filter(|r| {
            if let Some(target_lvl) = level {
                if r.level != target_lvl {
                    return false;
                }
            }
            if let Some(target_eid) = event_id {
                if r.event_id != target_eid {
                    return false;
                }
            }
            if let Some(needle) = &q {
                if r.record_id.to_string().contains(needle)
                    || r.event_id.to_string().contains(needle)
                    || r.title.to_ascii_lowercase().contains(needle)
                    || r.provider.to_ascii_lowercase().contains(needle)
                    || r.channel.to_ascii_lowercase().contains(needle)
                    || r.computer.to_ascii_lowercase().contains(needle)
                    || r.summary.to_ascii_lowercase().contains(needle)
                    || r.event_data.iter().any(|f| {
                        f.name.to_ascii_lowercase().contains(needle)
                            || f.value.to_ascii_lowercase().contains(needle)
                    })
                {
                    return true;
                }
                return false;
            }
            true
        })
        .collect();

    let filtered_total = filtered.len();
    let start = offset.min(filtered_total);
    let end = (start + limit).min(filtered_total);
    let paged: Vec<EvtxRecordItem> = filtered[start..end].iter().map(|&r| r.clone()).collect();

    EvtxPage {
        total,
        filtered_total,
        offset: start,
        limit,
        records: paged,
    }
}

struct RowMeta {
    record_id: u64,
    timestamp: String,
    level: u8,
    provider: String,
    channel: String,
    computer: String,
}

fn extract_row(xml: &str, record_id: u64, dt: &DateTime<chrono::Utc>) -> RowMeta {
    RowMeta {
        record_id,
        timestamp: dt.to_rfc3339(),
        level: extract_tag(xml, "<Level>", "</Level>")
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(4),
        provider: extract_provider_name(xml).unwrap_or_default(),
        channel: extract_tag(xml, "<Channel>", "</Channel>")
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
        computer: extract_tag(xml, "<Computer>", "</Computer>")
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
    }
}

fn extract_tag(haystack: &str, open: &str, close: &str) -> Option<String> {
    let start = haystack.find(open)? + open.len();
    let rel_end = haystack[start..].find(close)?;
    Some(haystack[start..start + rel_end].to_string())
}

fn extract_event_id(xml: &str) -> Option<u32> {
    if let Some(plain) = extract_tag(xml, "<EventID>", "</EventID>") {
        let inner = plain.trim();
        return inner.parse().ok();
    }
    if let Some(open_pos) = xml.find("<EventID ") {
        let after = &xml[open_pos..];
        let body_start = after.find('>')? + 1;
        let body_end = after[body_start..].find("</EventID>")?;
        return after[body_start..body_start + body_end].trim().parse().ok();
    }
    None
}

fn extract_provider_name(xml: &str) -> Option<String> {
    let key = "<Provider ";
    let start = xml.find(key)? + key.len();
    let close = xml[start..].find('>')? + start;
    let attrs = &xml[start..close];
    let name_key = "Name=\"";
    let name_start = attrs.find(name_key)? + name_key.len();
    let name_end = attrs[name_start..].find('"')?;
    Some(attrs[name_start..name_start + name_end].to_string())
}

fn parse_event_data(xml: &str) -> Vec<EventDataField> {
    let mut out: Vec<EventDataField> = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    #[derive(PartialEq)]
    enum Mode {
        Looking,
        EventData,
        UserData,
    }
    let mut mode = Mode::Looking;
    let mut unnamed: usize = 0;
    let mut current_name: Option<String> = None;
    let mut in_data = false;
    let mut text_buf = String::new();
    let mut stack: Vec<(String, bool, String)> = Vec::new();

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let local_q = e.local_name();
                let local = local_q.as_ref();
                match mode {
                    Mode::Looking => {
                        if local == "EventData" {
                            mode = Mode::EventData;
                            unnamed = 0;
                        } else if local == "UserData" {
                            mode = Mode::UserData;
                            stack.clear();
                        }
                    }
                    Mode::EventData => {
                        if local == "Data" && !in_data {
                            let mut name_attr: Option<String> = None;
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == "Name" {
                                    if let Ok(v) = attr.normalized_value(XmlVersion::Implicit1_0) {
                                        name_attr = Some(v.into_owned());
                                    }
                                }
                            }
                            current_name = Some(name_attr.unwrap_or_else(|| {
                                unnamed += 1;
                                format!("Data_{unnamed}")
                            }));
                            in_data = true;
                            text_buf.clear();
                        }
                    }
                    Mode::UserData => {
                        stack.push((local.to_string(), false, String::new()));
                    }
                }
            }
            Ok(Event::Text(t)) => {
                let txt: &str = &t;
                if mode == Mode::EventData && in_data {
                    text_buf.push_str(txt);
                } else if mode == Mode::UserData {
                    if let Some(top) = stack.last_mut() {
                        top.1 = true;
                        top.2.push_str(txt);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let local_q = e.local_name();
                let local = local_q.as_ref();
                match mode {
                    Mode::EventData => {
                        if local == "Data" && in_data {
                            if let Some(k) = current_name.take() {
                                out.push(EventDataField {
                                    name: k,
                                    value: text_buf.trim().to_string(),
                                });
                            }
                            in_data = false;
                            text_buf.clear();
                        } else if local == "EventData" {
                            mode = Mode::Looking;
                        }
                    }
                    Mode::UserData => {
                        if let Some((name, had_text, text)) = stack.pop() {
                            if had_text && !text.trim().is_empty() {
                                out.push(EventDataField {
                                    name,
                                    value: text.trim().to_string(),
                                });
                            }
                        }
                        if stack.is_empty() && local == "UserData" {
                            mode = Mode::Looking;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }

    out
}

fn get_field<'a>(fields: &'a [EventDataField], key: &str) -> Option<&'a str> {
    let lower = key.to_ascii_lowercase();
    for f in fields {
        if f.name.to_ascii_lowercase() == lower {
            return Some(&f.value);
        }
    }
    None
}

fn build_summary(event_id: u32, provider: &str, fields: &[EventDataField]) -> String {
    let prov = provider.to_ascii_lowercase();

    if event_id == 4624 {
        let user = get_field(fields, "TargetUserName").unwrap_or("-");
        let domain = get_field(fields, "TargetDomainName").unwrap_or("");
        let logon_type = get_field(fields, "LogonType").unwrap_or("-");
        let ip = get_field(fields, "IpAddress").unwrap_or("-");
        let full_user = if domain.is_empty() || domain == "-" {
            user.to_string()
        } else {
            format!("{domain}\\{user}")
        };
        let type_desc = match logon_type {
            "2" => "2 (交互式)",
            "3" => "3 (网络)",
            "4" => "4 (批处理)",
            "5" => "5 (服务)",
            "7" => "7 (解锁)",
            "8" => "8 (明文网络)",
            "9" => "9 (新凭据)",
            "10" => "10 (远程桌面 RDP)",
            "11" => "11 (缓存凭据)",
            _ => logon_type,
        };
        return format!("用户: {full_user} | 类型: {type_desc} | 来源 IP: {ip}");
    }

    if event_id == 4625 {
        let user = get_field(fields, "TargetUserName").unwrap_or("-");
        let status = get_field(fields, "Status").unwrap_or("-");
        let sub_status = get_field(fields, "SubStatus").unwrap_or("-");
        let ip = get_field(fields, "IpAddress").unwrap_or("-");
        return format!("登录失败 | 用户: {user} | 状态: {status} / {sub_status} | 来源 IP: {ip}");
    }

    if event_id == 4688 {
        let proc = get_field(fields, "NewProcessName").unwrap_or("-");
        let parent = get_field(fields, "ParentProcessName").unwrap_or("-");
        let cmd = get_field(fields, "CommandLine").unwrap_or("");
        let proc_leaf = proc.rsplit('\\').next().unwrap_or(proc);
        let parent_leaf = parent.rsplit('\\').next().unwrap_or(parent);
        if !cmd.is_empty() {
            return format!("进程: {proc_leaf} (父: {parent_leaf}) | 命令行: {cmd}");
        }
        return format!("进程: {proc_leaf} (父: {parent_leaf})");
    }

    if event_id == 7045 {
        let svc = get_field(fields, "ServiceName").unwrap_or("-");
        let img = get_field(fields, "ImagePath").unwrap_or("-");
        return format!("新服务: {svc} | 路径: {img}");
    }

    if event_id == 4104 {
        if let Some(script) = get_field(fields, "ScriptBlockText") {
            let clean = script.replace('\r', " ").replace('\n', " ");
            let snippet: String = clean.chars().take(100).collect();
            return format!("脚本块: {snippet}");
        }
    }

    if prov.contains("sysmon") {
        if event_id == 1 {
            let img = get_field(fields, "Image").unwrap_or("-");
            let cmd = get_field(fields, "CommandLine").unwrap_or("");
            return format!("创建进程: {img} | 命令行: {cmd}");
        }
        if event_id == 3 {
            let dst = get_field(fields, "DestinationIp").unwrap_or("-");
            let port = get_field(fields, "DestinationPort").unwrap_or("-");
            let img = get_field(fields, "Image").unwrap_or("-");
            return format!("网络连接: {img} -> {dst}:{port}");
        }
    }

    if event_id == 1102 || event_id == 104 {
        return "事件日志已被清除！".to_string();
    }

    let summary_parts: Vec<String> = fields
        .iter()
        .filter(|f| !f.value.is_empty() && f.value != "-" && f.value.len() < 80)
        .take(3)
        .map(|f| format!("{}: {}", f.name, f.value))
        .collect();

    if summary_parts.is_empty() {
        "-".to_string()
    } else {
        summary_parts.join(" | ")
    }
}

pub fn level_to_name(level: u8) -> String {
    match level {
        1 => "重大 (Critical)".to_string(),
        2 => "错误 (Error)".to_string(),
        3 => "警告 (Warning)".to_string(),
        4 => "信息 (Information)".to_string(),
        5 => "详细 (Verbose)".to_string(),
        _ => "常规".to_string(),
    }
}

pub fn event_title(provider: &str, event_id: u32) -> &'static str {
    let p = provider.to_ascii_lowercase();

    if p.contains("sysmon") {
        return match event_id {
            1 => "Sysmon: 进程创建 (Process Creation)",
            2 => "Sysmon: 更改文件创建时间 (File Creation Time Changed)",
            3 => "Sysmon: 网络连接已建立 (Network Connect)",
            5 => "Sysmon: 进程退出 (Process Terminated)",
            7 => "Sysmon: 驱动或模块加载 (Image Loaded)",
            8 => "Sysmon: 创建远程线程注入 (CreateRemoteThread)",
            9 => "Sysmon: 原始磁盘设备读取 (RawAccessRead)",
            10 => "Sysmon: 跨进程句柄访问 (ProcessAccess)",
            11 => "Sysmon: 文件创建 (FileCreate)",
            12 => "Sysmon: 注册表对象创建/删除 (Registry Create/Delete)",
            13 => "Sysmon: 注册表数值设置 (Registry Value Set)",
            14 => "Sysmon: 注册表键重命名 (Registry Rename)",
            15 => "Sysmon: 备用数据流创建 (FileCreateStreamHash)",
            17 => "Sysmon: 命名管道创建 (PipeCreated)",
            18 => "Sysmon: 命名管道连接 (PipeConnected)",
            19 => "Sysmon: WMI 过滤器创建 (WmiEventFilter)",
            20 => "Sysmon: WMI 消费者创建 (WmiEventConsumer)",
            21 => "Sysmon: WMI 绑定创建 (WmiEventConsumerToFilter)",
            22 => "Sysmon: DNS 域名解析查询 (DnsQuery)",
            _ => "Sysmon 诊断事件",
        };
    }

    if p.contains("powershell") {
        return match event_id {
            4103 => "PowerShell 模块执行日志 (Module Logging)",
            4104 => "PowerShell 脚本块执行日志 (Script Block)",
            400 => "PowerShell 引擎启动",
            600 => "PowerShell Provider 生命周期",
            _ => "PowerShell 执行事件",
        };
    }

    if p.contains("defender") {
        return match event_id {
            1116 => "Defender: 检出恶意软件 (Malware Detected)",
            1117 => "Defender: 恶意代码处理动作已执行",
            5001 => "Defender: 实时防护被关闭 (Real-time Protection Disabled)",
            5007 => "Defender: 防病毒配置被修改",
            _ => "Defender 安全防护事件",
        };
    }

    if p.contains("localsessionmanager") {
        return match event_id {
            21 => "RDP 会话登录成功 (Session Logon)",
            24 => "RDP 会话已断开 (Session Disconnect)",
            25 => "RDP 会话重新连接 (Session Reconnect)",
            _ => "本地会话管理器事件",
        };
    }

    if p.contains("remoteconnectionmanager") {
        return match event_id {
            1149 => "RDP 远程连接身份验证成功",
            _ => "远程连接管理器事件",
        };
    }

    if p.contains("taskscheduler") {
        return match event_id {
            106 => "计划任务已注册 (Task Registered)",
            140 => "计划任务已更新 (Task Updated)",
            200 => "计划任务开始执行 (Task Executed)",
            201 => "计划任务执行完成 (Task Completed)",
            _ => "任务计划程序事件",
        };
    }

    match event_id {
        104 => "系统/应用程序事件日志已被清除",
        1102 => "安全审计日志已被清除 (Audit Log Cleared)",
        1074 => "系统关机或重启 (Shutdown / Restart)",
        6005 => "事件日志服务已启动",
        6006 => "事件日志服务已正常停止",
        6008 => "系统非正常意外关机",
        41 => "系统内核电源异常重启 (Kernel-Power)",
        4608 => "Windows 正在启动",
        4609 => "Windows 正在关机",
        4616 => "系统时间被修改 (System Time Changed)",
        4624 => "账户登录成功 (Successful Logon)",
        4625 => "账户登录失败 (Failed Logon)",
        4634 => "账户注销 (Logoff)",
        4647 => "用户主动发起注销",
        4648 => "使用显式凭据登录 (RunAs)",
        4672 => "特权分配：授予超级管理员权限",
        4688 => "新进程已创建 (Process Creation)",
        4689 => "进程已退出 (Process Exit)",
        4697 => "系统中安装了新服务",
        4698 => "新建了计划任务",
        4699 => "删除了计划任务",
        4702 => "更新了计划任务",
        4719 => "系统审核策略被篡改 (Audit Policy Changed)",
        4720 => "创建了新用户账户 (User Created)",
        4722 => "启用了用户账户",
        4724 => "尝试重置用户密码",
        4728 => "将成员添加到全局安全组",
        4732 => "将成员添加到本地管理员组 (Administrators)",
        4738 => "用户账户属性被修改",
        4740 => "用户账户被锁定 (Account Locked Out)",
        4756 => "将成员添加到通用安全组",
        4768 => "Kerberos 身份验证票据 (TGT) 请求",
        4769 => "Kerberos 服务票据 (ST) 请求",
        4771 => "Kerberos 预身份验证失败 (密码喷洒/暴力破解)",
        4776 => "域控验证计算机/用户 NTLM 凭据",
        7036 => "服务运行状态变更",
        7040 => "服务启动类型配置变更",
        7045 => "系统安装新服务 (Service Installed)",
        _ => "Windows 系统事件",
    }
}
