//! Case-wide Cross-Image IOCs Hub & Automatic Case Wordlist Generator (P1-7).
//!
//! Aggregates forensic indicators (IPv4, MAC, Email, Hostname/Domain, Accounts,
//! WiFi SSID, USB/Disk Serials, and Crypto/API tokens) across all published
//! plugin runs in `records.db` and loaded evidence sessions, highlighting
//! multi-image hits (`cross_image_hit: true`), and generates a prioritized
//! case-specific password wordlist.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use regex::Regex;
use serde::Serialize;

use crate::core::analysis::results::record_query::{self, RecordsQuery};
use crate::core::filesystem::evidence_session::EvidenceSession;
use crate::core::filesystem::tree::fs_tree::{self, NodeKind};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseIocItem {
    pub kind: String, // "ip" | "mac" | "email" | "domain" | "account" | "ssid" | "serial" | "token"
    pub value: String,
    pub count: usize,
    pub cross_image_hit: bool,
    pub images: Vec<String>,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseIocsReport {
    pub total_iocs: usize,
    pub cross_image_count: usize,
    pub by_kind: BTreeMap<String, usize>,
    pub items: Vec<CaseIocItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseWordlistReport {
    pub base_count: usize,
    pub total_with_variants: usize,
    pub exported_path: Option<String>,
    pub words: Vec<String>,
    pub sources: Vec<String>,
}

#[derive(Default)]
struct IocEntry {
    count: usize,
    images: BTreeSet<String>,
    sources: BTreeSet<String>,
}

type IocMap = BTreeMap<(String, String), IocEntry>;

struct IocAccumulator {
    entries: IocMap,
    ip_re: Option<Regex>,
    mac_re: Option<Regex>,
    email_re: Option<Regex>,
    token_re: Option<Regex>,
}

impl IocAccumulator {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            ip_re: Regex::new(r"\b(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\b").ok(),
            mac_re: Regex::new(r"(?i)\b(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}\b").ok(),
            email_re: Regex::new(r"(?i)\b[a-z0-9._%+-]{2,64}@[a-z0-9.-]{2,64}\.[a-z]{2,12}\b").ok(),
            token_re: Regex::new(r"\b(?:gAAAAA[A-Za-z0-9_-]{60,}|gh[pousr]_[A-Za-z0-9]{20,}|sk-[A-Za-z0-9]{20,}|AKIA[0-9A-Z]{16})\b").ok(),
        }
    }

    fn record(&mut self, kind: &str, value: &str, image_label: &str, source: &str) {
        Self::record_into(&mut self.entries, kind, value, image_label, source);
    }

    fn record_into(entries: &mut IocMap, kind: &str, value: &str, image_label: &str, source: &str) {
        let clean = value.trim();
        if clean.len() < 2 || clean.len() > 256 {
            return;
        }
        let entry = entries
            .entry((kind.to_string(), clean.to_string()))
            .or_default();
        entry.count += 1;
        entry.images.insert(image_label.to_string());
        if entry.sources.len() < 8 {
            entry.sources.insert(source.to_string());
        }
    }

    fn scan_text(&mut self, text: &str, image_label: &str, source: &str) {
        if let Some(ref ip_re) = self.ip_re {
            for m in ip_re.find_iter(text) {
                let ip = m.as_str();
                if !matches!(
                    ip,
                    "0.0.0.0" | "127.0.0.1" | "255.255.255.255" | "255.255.255.0"
                ) && !ip.starts_with("0.")
                {
                    Self::record_into(&mut self.entries, "ip", ip, image_label, source);
                }
            }
        }
        if let Some(ref mac_re) = self.mac_re {
            for m in mac_re.find_iter(text) {
                let mac = m.as_str().to_uppercase().replace('-', ":");
                if mac != "00:00:00:00:00:00" && mac != "FF:FF:FF:FF:FF:FF" {
                    Self::record_into(&mut self.entries, "mac", &mac, image_label, source);
                }
            }
        }
        if let Some(ref email_re) = self.email_re {
            for m in email_re.find_iter(text) {
                Self::record_into(&mut self.entries, "email", m.as_str(), image_label, source);
            }
        }
        if let Some(ref token_re) = self.token_re {
            for m in token_re.find_iter(text) {
                Self::record_into(&mut self.entries, "token", m.as_str(), image_label, source);
            }
        }
    }

    fn finish(self, filter_kind: Option<&str>, query: Option<&str>) -> CaseIocsReport {
        let q_lower = query
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);
        let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
        let mut items = Vec::new();
        let mut cross_image_count = 0usize;

        for ((kind, value), entry) in self.entries {
            let IocEntry {
                count,
                images,
                sources,
            } = entry;
            *by_kind.entry(kind.clone()).or_insert(0) += 1;
            let cross = images.len() > 1;
            if cross {
                cross_image_count += 1;
            }
            if let Some(fk) = filter_kind.filter(|s| !s.is_empty()) {
                if kind != fk {
                    continue;
                }
            }
            if let Some(ref q) = q_lower {
                if !value.to_lowercase().contains(q)
                    && !sources.iter().any(|s| s.to_lowercase().contains(q))
                {
                    continue;
                }
            }
            items.push(CaseIocItem {
                kind,
                value,
                count,
                cross_image_hit: cross,
                images: images.into_iter().collect(),
                sources: sources.into_iter().collect(),
            });
        }

        items.sort_by(|a, b| {
            b.cross_image_hit
                .cmp(&a.cross_image_hit)
                .then(b.count.cmp(&a.count))
                .then(a.kind.cmp(&b.kind))
                .then(a.value.cmp(&b.value))
        });

        let total_iocs = items.len();
        CaseIocsReport {
            total_iocs,
            cross_image_count,
            by_kind,
            items,
        }
    }
}

/// Build the Cross-Image IOCs Report from case `records.db` stores and loaded `EvidenceSession`s.
pub fn build_case_iocs(
    case_dir: Option<&Path>,
    evidence_ids: &[(String, String)], // (image_id, label)
    loaded_images: &[(String, Arc<EvidenceSession>)],
    filter_kind: Option<&str>,
    query: Option<&str>,
) -> CaseIocsReport {
    let mut acc = IocAccumulator::new();

    if let Some(cdir) = case_dir {
        for (image_id, label) in evidence_ids {
            if let Ok(Some(conn)) = record_query::open(cdir, image_id) {
                harvest_iocs_from_records_db(&conn, label, &mut acc);
            }
        }
    }

    for (image_id, session) in loaded_images {
        let label = evidence_ids
            .iter()
            .find(|(id, _)| id == image_id)
            .map(|(_, l)| l.as_str())
            .unwrap_or(image_id.as_str());
        harvest_iocs_from_live_image(session, label, &mut acc);
    }

    acc.finish(filter_kind, query)
}

fn harvest_iocs_from_records_db(
    conn: &rusqlite::Connection,
    image_label: &str,
    acc: &mut IocAccumulator,
) {
    let Ok(runs) = record_query::published_runs(conn) else {
        return;
    };
    for run in runs {
        let Ok(rtypes) = record_query::run_record_types(conn, &run.run_id) else {
            continue;
        };
        for rt in rtypes {
            let Ok(page) = record_query::records_page(
                conn,
                &RecordsQuery {
                    run_id: &run.run_id,
                    record_type: &rt.schema.record_type,
                    keyword: None,
                    filters: &[],
                    bcp_scope: None,
                    sort: None,
                    descending: false,
                    skip: 0,
                    limit: 500,
                },
            ) else {
                continue;
            };
            let src = format!("{}:{}", run.analyzer_name, rt.schema.label);
            for item in page.items {
                acc.scan_text(&item.title, image_label, &src);
                for (field_id, val) in &item.values {
                    let Some(s) = val.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
                        continue;
                    };
                    let fid = field_id.to_ascii_lowercase();
                    if matches!(fid.as_str(), "username" | "user" | "account" | "user_name") {
                        acc.record("account", s, image_label, &src);
                    } else if fid.contains("ssid") {
                        acc.record("ssid", s, image_label, &src);
                    } else if fid.contains("serial") {
                        acc.record("serial", s, image_label, &src);
                    } else if fid.contains("hostname") || fid.contains("domain") || fid == "host" {
                        acc.record("domain", s, image_label, &src);
                    }
                    acc.scan_text(s, image_label, &src);
                }
            }
        }
    }
}

fn harvest_iocs_from_live_image(
    image: &Arc<EvidenceSession>,
    image_label: &str,
    acc: &mut IocAccumulator,
) {
    let Ok(roots) = fs_tree::children_for_image(image, "root") else {
        return;
    };
    for root in roots
        .into_iter()
        .filter(|r| r.kind == NodeKind::Dir)
        .take(8)
    {
        let Ok(top) = fs_tree::children_for_image(image, &root.id) else {
            continue;
        };
        for node in top {
            let lower = node.name.to_ascii_lowercase();
            if lower == "users" && node.kind == NodeKind::Dir {
                if let Ok(users) = fs_tree::children_for_image(image, &node.id) {
                    for u in users.into_iter().filter(|u| u.kind == NodeKind::Dir) {
                        if !matches!(
                            u.name.to_ascii_lowercase().as_str(),
                            "public" | "default" | "default user" | "all users"
                        ) {
                            acc.record("account", &u.name, image_label, "Users 目录");
                        }
                    }
                }
            } else if lower == "etc" && node.kind == NodeKind::Dir {
                if let Ok(etc_files) = fs_tree::children_for_image(image, &node.id) {
                    for f in etc_files {
                        if f.name == "hostname" {
                            if let Ok(b) = fs_tree::read_file_limited(image, &f.id, 1024) {
                                let h = String::from_utf8_lossy(&b).trim().to_string();
                                if !h.is_empty() {
                                    acc.record("domain", &h, image_label, "/etc/hostname");
                                }
                            }
                        } else if f.name == "hosts" {
                            if let Ok(b) = fs_tree::read_file_limited(image, &f.id, 32768) {
                                acc.scan_text(
                                    &String::from_utf8_lossy(&b),
                                    image_label,
                                    "/etc/hosts",
                                );
                            }
                        } else if f.name == "passwd" {
                            if let Ok(b) = fs_tree::read_file_limited(image, &f.id, 65536) {
                                for line in String::from_utf8_lossy(&b).lines() {
                                    let parts: Vec<&str> = line.split(':').collect();
                                    if parts.len() >= 7 {
                                        let uid = parts[2].parse::<u32>().unwrap_or(99);
                                        if uid == 0 || (1000..65534).contains(&uid) {
                                            acc.record(
                                                "account",
                                                parts[0],
                                                image_label,
                                                "/etc/passwd",
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Generate a prioritized Case Wordlist harvesting password hints, usernames,
/// hostnames, SSIDs, and note tokens from the active case and loaded images.
pub fn generate_case_wordlist(
    case_dir: Option<&Path>,
    evidence_ids: &[(String, String)],
    loaded_images: &[(String, Arc<EvidenceSession>)],
    export_path: Option<&str>,
) -> anyhow::Result<CaseWordlistReport> {
    let mut base_seeds: Vec<String> = Vec::new();
    let mut seen_base = BTreeSet::new();
    let mut sources = BTreeSet::new();

    let mut push_seed = |word: &str, src: &str| {
        let clean = word.trim();
        if (2..=64).contains(&clean.len())
            && !clean.chars().any(char::is_control)
            && seen_base.insert(clean.to_string())
        {
            base_seeds.push(clean.to_string());
            sources.insert(src.to_string());
        }
    };

    // 1. Harvest from case records.db (password hints, credentials, usernames, SSIDs, hostnames)
    if let Some(cdir) = case_dir {
        for (image_id, label) in evidence_ids {
            if let Ok(Some(conn)) = record_query::open(cdir, image_id) {
                if let Ok(runs) = record_query::published_runs(&conn) {
                    for run in runs {
                        let Ok(rtypes) = record_query::run_record_types(&conn, &run.run_id) else {
                            continue;
                        };
                        for rt in rtypes {
                            let Ok(page) = record_query::records_page(
                                &conn,
                                &RecordsQuery {
                                    run_id: &run.run_id,
                                    record_type: &rt.schema.record_type,
                                    keyword: None,
                                    filters: &[],
                                    bcp_scope: None,
                                    sort: None,
                                    descending: false,
                                    skip: 0,
                                    limit: 500,
                                },
                            ) else {
                                continue;
                            };
                            for item in page.items {
                                for (k, v) in &item.values {
                                    let Some(s) =
                                        v.as_str().map(str::trim).filter(|s| !s.is_empty())
                                    else {
                                        continue;
                                    };
                                    let kl = k.to_ascii_lowercase();
                                    if kl.contains("password")
                                        || kl.contains("hint")
                                        || kl.contains("secret")
                                        || kl.contains("pass")
                                        || kl.contains("ssid")
                                        || kl.contains("username")
                                        || kl.contains("hostname")
                                    {
                                        push_seed(s, &format!("{label}:{}", rt.schema.label));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Harvest from live images (Users, Desktop/Documents small .txt/.md/.env files, /etc/hostname)
    for (_id, session) in loaded_images {
        let Ok(roots) = fs_tree::children_for_image(session, "root") else {
            continue;
        };
        for root in roots
            .into_iter()
            .filter(|r| r.kind == NodeKind::Dir)
            .take(8)
        {
            let Ok(top) = fs_tree::children_for_image(session, &root.id) else {
                continue;
            };
            for node in top {
                let lower = node.name.to_ascii_lowercase();
                if lower == "users" && node.kind == NodeKind::Dir {
                    if let Ok(users) = fs_tree::children_for_image(session, &node.id) {
                        for u in users.into_iter().filter(|u| u.kind == NodeKind::Dir) {
                            if !matches!(
                                u.name.to_ascii_lowercase().as_str(),
                                "public" | "default" | "default user" | "all users"
                            ) {
                                push_seed(&u.name, "Windows Users");
                                harvest_user_text_clues(session, &u.id, &mut push_seed);
                            }
                        }
                    }
                } else if (lower == "root" || lower == "home") && node.kind == NodeKind::Dir {
                    harvest_user_text_clues(session, &node.id, &mut push_seed);
                }
            }
        }
    }

    let base_count = base_seeds.len();
    let mut words = Vec::with_capacity(base_count * 8 + 32);
    let mut seen_all = BTreeSet::new();

    for seed in &base_seeds {
        for cand in [
            seed.clone(),
            seed.to_lowercase(),
            seed.to_uppercase(),
            format!("{seed}123"),
            format!("{seed}@123"),
            format!("{seed}!"),
            format!("{seed}2024"),
            format!("{seed}2025"),
            format!("{seed}2026"),
            format!("{seed}888"),
            format!("123{seed}"),
        ] {
            if seen_all.insert(cand.clone()) {
                words.push(cand);
            }
        }
    }

    let exported_path = if let Some(path) = export_path.map(str::trim).filter(|s| !s.is_empty()) {
        let dest = Path::new(path);
        if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, words.join("\n"))?;
        Some(path.to_string())
    } else {
        None
    };

    Ok(CaseWordlistReport {
        base_count,
        total_with_variants: words.len(),
        exported_path,
        words,
        sources: sources.into_iter().collect(),
    })
}

fn harvest_user_text_clues(
    image: &Arc<EvidenceSession>,
    user_dir_id: &str,
    push_seed: &mut impl FnMut(&str, &str),
) {
    let Ok(children) = fs_tree::children_for_image(image, user_dir_id) else {
        return;
    };
    for c in children {
        if c.kind == NodeKind::Dir
            && matches!(
                c.name.to_ascii_lowercase().as_str(),
                "desktop" | "documents" | "downloads"
            )
        {
            if let Ok(sub) = fs_tree::children_for_image(image, &c.id) {
                for f in sub
                    .into_iter()
                    .filter(|f| f.kind == NodeKind::File && f.size > 0 && f.size <= 32768)
                    .take(20)
                {
                    scan_small_clue_file(image, &f, push_seed);
                }
            }
        } else if c.kind == NodeKind::File && c.size > 0 && c.size <= 32768 {
            scan_small_clue_file(image, &c, push_seed);
        }
    }
}

fn scan_small_clue_file(
    image: &Arc<EvidenceSession>,
    file: &fs_tree::FsNode,
    push_seed: &mut impl FnMut(&str, &str),
) {
    let lower = file.name.to_ascii_lowercase();
    if !(lower.ends_with(".txt")
        || lower.ends_with(".md")
        || lower.ends_with(".ini")
        || lower.ends_with(".env")
        || lower.contains("pass")
        || lower.contains("pwd")
        || lower.contains("key")
        || lower.contains("secret")
        || lower.contains("hint"))
    {
        return;
    }
    let Ok(bytes) = fs_tree::read_file_limited(image, &file.id, 32768) else {
        return;
    };
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines().take(100) {
        let trimmed = line.trim();
        if let Some((_k, v)) = trimmed.split_once([':', '=']) {
            let val = v.trim().trim_matches(['"', '\'']);
            if (3..=48).contains(&val.len()) && !val.contains(' ') {
                push_seed(val, &format!("文件线索:{}", file.name));
            }
        } else if (4..=32).contains(&trimmed.len()) && !trimmed.contains(' ') {
            push_seed(trimmed, &format!("文件线索:{}", file.name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioc_accumulator_extracts_and_flags_cross_image_hits() {
        let mut acc = IocAccumulator::new();
        acc.scan_text(
            "Connection from 192.168.1.50 mac 00-11-22-33-44-55 user admin@corp.local",
            "img1",
            "syslog",
        );
        acc.scan_text("Target 192.168.1.50 seen again", "img2", "pcap");

        let report = acc.finish(None, None);
        let ip_item = report
            .items
            .iter()
            .find(|i| i.kind == "ip" && i.value == "192.168.1.50")
            .unwrap();
        assert!(ip_item.cross_image_hit);
        assert_eq!(ip_item.images.len(), 2);
        assert!(report
            .items
            .iter()
            .any(|i| i.kind == "mac" && i.value == "00:11:22:33:44:55"));
        assert!(report
            .items
            .iter()
            .any(|i| i.kind == "email" && i.value == "admin@corp.local"));
    }
}
