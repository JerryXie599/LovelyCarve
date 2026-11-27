//! query implementation.
use super::*;

pub(super) fn read_rows(
    session: &SqliteSession,
    key: &SqliteKey,
    table: &str,
    offset: i64,
    limit: i64,
    order_by: Option<&str>,
    descending: bool,
) -> anyhow::Result<SqliteRows> {
    let (conn, _encrypted) = open_session_conn(session, key)?;

    read_rows_from_conn(&conn, table, offset, limit, order_by, descending)
}

pub(super) fn read_rows_from_conn(
    conn: &Connection,
    table: &str,
    offset: i64,
    limit: i64,
    order_by: Option<&str>,
    descending: bool,
) -> anyhow::Result<SqliteRows> {
    crate::core::preview::sqlite::sqlite_preview_budget::run(conn, || {
        let columns = read_columns(conn, table)?;
        let total = count_rows(conn, table)?;
        let offset = offset.max(0);
        let limit = limit.clamp(1, MAX_PAGE_ROWS);

        let order_clause = match order_by {
            Some(col) if columns.iter().any(|c| c.name == col) => {
                let dir = if descending { "DESC" } else { "ASC" };
                format!(" ORDER BY {} {dir}", quote_ident(col))
            }
            _ => String::new(),
        };
        let sql = format!(
            "SELECT * FROM {}{order_clause} LIMIT {limit} OFFSET {offset}",
            quote_ident(table)
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut query = stmt.query([])?;
        let mut rows: Vec<Vec<SqliteCell>> = Vec::new();
        while let Some(row) = query.next()? {
            let mut cells = Vec::with_capacity(columns.len());
            for index in 0..columns.len() {
                let cell = match row.get_ref(index)? {
                    ValueRef::Null => SqliteCell::Null,
                    ValueRef::Integer(value) => SqliteCell::Integer(value),
                    ValueRef::Real(value) => SqliteCell::Real(value),
                    ValueRef::Text(bytes) => {
                        SqliteCell::Text(truncate_text(&String::from_utf8_lossy(bytes)))
                    }
                    ValueRef::Blob(bytes) => blob_cell(bytes),
                };
                cells.push(cell);
            }
            rows.push(cells);
        }
        let returned = rows.len();
        Ok(SqliteRows {
            columns,
            rows,
            total,
            offset,
            returned,
        })
    })
}

pub(super) fn search_database(
    session: &SqliteSession,
    key: &SqliteKey,
    pattern: &str,
    is_regex: bool,
    limit: usize,
) -> anyhow::Result<SqliteSearchResult> {
    let (conn, _encrypted) = open_session_conn(session, key)?;
    search_database_from_conn(&conn, pattern, is_regex, limit)
}

pub(super) fn search_database_from_conn(
    conn: &Connection,
    pattern: &str,
    is_regex: bool,
    limit: usize,
) -> anyhow::Result<SqliteSearchResult> {
    let pat = pattern.trim();
    if pat.is_empty() {
        anyhow::bail!("搜索关键词不能为空");
    }
    let cap = limit.clamp(1, 2_000);
    let compiled_re = if is_regex {
        Some(
            regex::RegexBuilder::new(pat)
                .case_insensitive(true)
                .build()
                .map_err(|e| anyhow::anyhow!("无效的正则表达式：{e}"))?,
        )
    } else {
        None
    };
    let pat_lower = pat.to_lowercase();
    let matches_str = |s: &str| -> bool {
        if let Some(re) = &compiled_re {
            re.is_match(s)
        } else {
            s.to_lowercase().contains(&pat_lower)
        }
    };

    crate::core::preview::sqlite::sqlite_preview_budget::run(conn, || {
        let tables: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name COLLATE NOCASE",
            )?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut hits = Vec::new();
        let mut truncated = false;

        'tables: for table in tables {
            let Ok(columns) = read_columns(conn, &table) else {
                continue;
            };
            if columns.is_empty() {
                continue;
            }
            let sql = format!("SELECT * FROM {}", quote_ident(&table));
            let Ok(mut stmt) = conn.prepare(&sql) else {
                continue;
            };
            let Ok(mut q) = stmt.query([]) else {
                continue;
            };
            let mut row_idx = 0i64;
            while let Ok(Some(row)) = q.next() {
                row_idx += 1;
                let mut cells = Vec::with_capacity(columns.len());
                let mut hit_col: Option<(String, String)> = None;
                for (ci, col) in columns.iter().enumerate() {
                    let Ok(vref) = row.get_ref(ci) else {
                        cells.push(SqliteCell::Null);
                        continue;
                    };
                    let cell = match vref {
                        ValueRef::Null => SqliteCell::Null,
                        ValueRef::Integer(v) => {
                            let s = v.to_string();
                            if hit_col.is_none() && matches_str(&s) {
                                hit_col = Some((col.name.clone(), s));
                            }
                            SqliteCell::Integer(v)
                        }
                        ValueRef::Real(v) => {
                            let s = v.to_string();
                            if hit_col.is_none() && matches_str(&s) {
                                hit_col = Some((col.name.clone(), s));
                            }
                            SqliteCell::Real(v)
                        }
                        ValueRef::Text(b) => {
                            let full = String::from_utf8_lossy(b);
                            if hit_col.is_none() && matches_str(&full) {
                                hit_col = Some((col.name.clone(), truncate_text(&full)));
                            }
                            SqliteCell::Text(truncate_text(&full))
                        }
                        ValueRef::Blob(b) => {
                            let bc = blob_cell(b);
                            if hit_col.is_none() {
                                if let SqliteCell::Blob {
                                    decoded: Some(ref dec),
                                    ..
                                } = bc
                                {
                                    let detail_str = dec.detail.to_string();
                                    if matches_str(&dec.summary) || matches_str(&detail_str) {
                                        hit_col = Some((col.name.clone(), dec.summary.clone()));
                                    }
                                }
                            }
                            bc
                        }
                    };
                    cells.push(cell);
                }
                if let Some((col_name, matched_value)) = hit_col {
                    if hits.len() >= cap {
                        truncated = true;
                        break 'tables;
                    }
                    hits.push(SqliteSearchHit {
                        table: table.clone(),
                        row_index: row_idx,
                        column: col_name,
                        matched_value,
                        cells,
                    });
                }
            }
        }

        let total_hits = hits.len();
        Ok(SqliteSearchResult {
            query: pat.to_string(),
            total_hits,
            truncated,
            hits,
        })
    })
}

#[cfg(test)]
pub(super) fn read_rows_path(
    path: &Path,
    key: &SqliteKey,
    table: &str,
    offset: i64,
    limit: i64,
    order_by: Option<&str>,
    descending: bool,
) -> anyhow::Result<SqliteRows> {
    let (conn, _encrypted) = open_path_conn(path, key)?;
    read_rows_from_conn(&conn, table, offset, limit, order_by, descending)
}
