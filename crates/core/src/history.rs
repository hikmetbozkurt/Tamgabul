//! Conversation history in SQLite (P3 core, docs/06). Results are stored as
//! references (document + chunk), never as copied text, and are rebuilt and
//! re-authorized every time they are shown — a revoked folder's results
//! disappear from old conversations too.
//!
//! With history off, sessions are marked not persisted and removed at the
//! next start.

use rusqlite::{params, Connection, OptionalExtension};

use crate::dto::{HistorySession, HistoryTurn, SearchKind, TurnRole};
use crate::error::{ErrorCode, IpcError};
use crate::index::store::new_id;
use crate::search::{self, Acl, Hit};

fn err(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, format!("history: {e}"))
}

pub fn ensure_session(
    conn: &Connection,
    id: Option<&str>,
    persisted: bool,
) -> Result<String, IpcError> {
    if let Some(id) = id {
        let exists: Option<String> = conn
            .query_row(
                "SELECT session_id FROM sessions WHERE session_id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if exists.is_some() {
            conn.execute(
                "UPDATE sessions SET last_active_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE session_id = ?1",
                [id],
            )
            .map_err(err)?;
            return Ok(id.to_string());
        }
    }
    let id = new_id("c", "session");
    conn.execute(
        "INSERT INTO sessions(session_id, persisted) VALUES (?1, ?2)",
        params![id, persisted as i64],
    )
    .map_err(err)?;
    Ok(id)
}

fn next_ordinal(conn: &Connection, session: &str) -> Result<i64, IpcError> {
    conn.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM turns WHERE session_id = ?1",
        [session],
        |r| r.get(0),
    )
    .map_err(err)
}

pub struct ShownResult<'a> {
    pub hit: &'a Hit,
    pub scope_id: Option<String>,
    pub title: String,
}

/// Store one exchange: the user's text and the assistant answer with its
/// result references. Returns the assistant turn id.
pub fn record_search(
    conn: &mut Connection,
    session: &str,
    user_text: &str,
    kind: SearchKind,
    results: &[ShownResult<'_>],
    generation: Option<&str>,
) -> Result<String, IpcError> {
    let tx = conn.transaction().map_err(err)?;
    let ord = next_ordinal(&tx, session)?;
    let user_turn = new_id("u", session);
    tx.execute(
        "INSERT INTO turns(turn_id, session_id, ordinal, role, input_mode, text, intent)
         VALUES (?1, ?2, ?3, 'user', 'typed', ?4, 'search_current')",
        params![user_turn, session, ord, user_text],
    )
    .map_err(err)?;
    let ai_turn = new_id("a", session);
    let reasons: Vec<&str> = results.iter().map(|r| r.hit.reason.as_str()).collect();
    let slots = serde_json::json!({
        "kind": kind,
        "count": results.len(),
        "reasons": reasons,
    });
    tx.execute(
        "INSERT INTO turns(turn_id, session_id, ordinal, role, intent, slots_json)
         VALUES (?1, ?2, ?3, 'assistant', 'search_current', ?4)",
        params![ai_turn, session, ord + 1, slots.to_string()],
    )
    .map_err(err)?;
    let event = new_id("e", session);
    tx.execute(
        "INSERT INTO search_events(event_id, session_id, turn_id, event_type, query_terms, generation_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            event,
            session,
            ai_turn,
            if results.is_empty() { "search_failed" } else { "results_shown" },
            user_text,
            generation
        ],
    )
    .map_err(err)?;
    for (rank, r) in results.iter().enumerate() {
        tx.execute(
            "INSERT INTO result_interactions(interaction_id, event_id, document_id, chunk_id, scope_id, rank, score, action, title_snapshot)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'shown', ?8)",
            params![
                new_id("i", &r.hit.document_id),
                event,
                r.hit.document_id,
                r.hit.chunk_id,
                r.scope_id,
                rank as i64,
                r.hit.score,
                r.title
            ],
        )
        .map_err(err)?;
    }
    tx.execute(
        "UPDATE sessions SET last_active_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE session_id = ?1",
        [session],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(ai_turn)
}

/// Note that a shown result was opened (for "opened" badges and history).
pub fn mark_opened(conn: &Connection, document_id: &str) -> Result<(), IpcError> {
    type Shown = (String, Option<i64>, Option<String>, Option<String>);
    let row: Option<Shown> = conn
        .query_row(
            "SELECT event_id, chunk_id, scope_id, title_snapshot FROM result_interactions
             WHERE document_id = ?1 AND action = 'shown' ORDER BY created_at DESC LIMIT 1",
            [document_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(err)?;
    if let Some((event, chunk, scope, title)) = row {
        conn.execute(
            "INSERT INTO result_interactions(interaction_id, event_id, document_id, chunk_id, scope_id, action, title_snapshot)
             VALUES (?1, ?2, ?3, ?4, ?5, 'opened', ?6)",
            params![new_id("i", document_id), event, document_id, chunk, scope, title],
        )
        .map_err(err)?;
    }
    Ok(())
}

pub fn session_turns(conn: &Connection, session: &str) -> Result<Vec<HistoryTurn>, IpcError> {
    let acl = Acl::load(conn)?;
    let mut st = conn
        .prepare(
            "SELECT t.turn_id, t.role, t.created_at, t.text, t.slots_json,
                (SELECT event_id FROM search_events e WHERE e.turn_id = t.turn_id LIMIT 1)
             FROM turns t WHERE t.session_id = ?1 ORDER BY t.ordinal",
        )
        .map_err(err)?;
    type Row = (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row> = st
        .query_map([session], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut out = Vec::new();
    let mut last_query = String::new();
    for (id, role, at, text, slots, event) in rows {
        if role == "user" {
            last_query = text.clone().unwrap_or_default();
            out.push(HistoryTurn {
                turn_id: id,
                role: TurnRole::User,
                at,
                text,
                kind: None,
                count: None,
                choices: Vec::new(),
                results: Vec::new(),
                opened: false,
            });
            continue;
        }
        let slots: serde_json::Value = slots
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let reasons: Vec<String> = slots["reasons"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let terms = search::terms(&last_query);
        let mut results = Vec::new();
        let mut opened = false;
        if let Some(event) = &event {
            let mut st = conn
                .prepare(
                    "SELECT document_id, chunk_id, rank FROM result_interactions
                     WHERE event_id = ?1 AND action = 'shown' AND is_tombstone = 0 AND document_id IS NOT NULL
                     ORDER BY rank",
                )
                .map_err(err)?;
            let shown: Vec<(String, Option<i64>, Option<i64>)> = st
                .query_map([event], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(err)?
                .collect::<Result<_, _>>()
                .map_err(err)?;
            for (doc, chunk, rank) in shown {
                let reason = rank
                    .and_then(|r| reasons.get(r as usize))
                    .map(String::as_str)
                    .unwrap_or("semantic");
                if let Some(r) = search::resolve(conn, &acl, &doc, chunk, reason, &terms)? {
                    results.push(r.card);
                }
            }
            opened = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM result_interactions WHERE event_id = ?1 AND action = 'opened')",
                    [event],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(err)?
                != 0;
        }
        let kind = if results.is_empty() {
            SearchKind::NotFound
        } else {
            SearchKind::Results
        };
        out.push(HistoryTurn {
            turn_id: id,
            role: TurnRole::Assistant,
            at,
            text: None,
            kind: Some(kind),
            count: Some(results.len() as u32),
            choices: Vec::new(),
            results,
            opened,
        });
    }
    Ok(out)
}

pub fn list_sessions(conn: &Connection) -> Result<Vec<HistorySession>, IpcError> {
    let mut st = conn
        .prepare(
            "SELECT s.session_id, s.started_at,
                (SELECT text FROM turns t WHERE t.session_id = s.session_id AND t.role = 'user' ORDER BY ordinal LIMIT 1),
                (SELECT COUNT(*) FROM result_interactions i JOIN search_events e ON e.event_id = i.event_id
                   WHERE e.session_id = s.session_id AND i.action = 'shown' AND i.is_tombstone = 0),
                (SELECT COUNT(*) FROM result_interactions i JOIN search_events e ON e.event_id = i.event_id
                   WHERE e.session_id = s.session_id AND i.action = 'opened' AND i.is_tombstone = 0)
             FROM sessions s WHERE s.persisted = 1
               AND EXISTS (SELECT 1 FROM turns t WHERE t.session_id = s.session_id)
             ORDER BY s.last_active_at DESC LIMIT 200",
        )
        .map_err(err)?;
    let v = st
        .query_map([], |r| {
            let title: Option<String> = r.get(2)?;
            Ok(HistorySession {
                session_id: r.get(0)?,
                started_at: r.get(1)?,
                title: title
                    .map(|t| t.chars().take(80).collect())
                    .unwrap_or_default(),
                result_count: r.get(3)?,
                opened_count: r.get(4)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    Ok(v)
}

pub fn delete_session(conn: &Connection, session: &str) -> Result<(), IpcError> {
    // Turns, events and interactions go with it (ON DELETE CASCADE).
    conn.execute("DELETE FROM sessions WHERE session_id = ?1", [session])
        .map_err(err)?;
    Ok(())
}

pub fn clear_all(conn: &Connection) -> Result<(), IpcError> {
    conn.execute("DELETE FROM sessions", []).map_err(err)?;
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok(())
}

/// App start: drop sessions recorded while history was off.
pub fn purge_unpersisted(conn: &Connection) -> Result<(), IpcError> {
    conn.execute("DELETE FROM sessions WHERE persisted = 0", [])
        .map_err(err)?;
    Ok(())
}
