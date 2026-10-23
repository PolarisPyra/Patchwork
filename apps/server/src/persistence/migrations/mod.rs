//! Schema migrations: numbered SQL files in this folder, applied in order inside one
//! transaction each.
//!
//! The `schema_migrations` table records what has run (id, name, and when, in Unix seconds)
//! and is what gets checked: a database whose history is not a prefix of the list below was
//! written by a different build, and is refused rather than guessed at. `PRAGMA user_version`
//! is kept equal to the newest id, so an older build can still tell how far the database got.
//!
//! To change the schema, add the next numbered file and list it in `MIGRATIONS`. Never edit
//! or renumber a released one.

use rusqlite::{Connection, params};

use super::codec::Res;

struct Migration {
    id: u32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        id: 1,
        name: "initial_schema",
        sql: include_str!("0001_initial_schema.sql"),
    },
    Migration {
        id: 2,
        name: "access_mode_and_approvals",
        sql: include_str!("0002_access_mode_and_approvals.sql"),
    },
    Migration {
        id: 3,
        name: "message_attachments",
        sql: include_str!("0003_message_attachments.sql"),
    },
    Migration {
        id: 4,
        name: "message_steer",
        sql: include_str!("0004_message_steer.sql"),
    },
    Migration {
        id: 5,
        name: "thread_goal",
        sql: include_str!("0005_thread_goal.sql"),
    },
    Migration {
        id: 6,
        name: "thread_position",
        sql: include_str!("0006_thread_position.sql"),
    },
    Migration {
        id: 7,
        name: "folders",
        sql: include_str!("0007_folders.sql"),
    },
    Migration {
        id: 8,
        name: "app_state",
        sql: include_str!("0008_app_state.sql"),
    },
    Migration {
        id: 9,
        name: "thread_effort",
        sql: include_str!("0009_thread_effort.sql"),
    },
    Migration {
        id: 10,
        name: "message_finished_at",
        sql: include_str!("0010_message_finished_at.sql"),
    },
    Migration {
        id: 11,
        name: "message_usage",
        sql: include_str!("0011_message_usage.sql"),
    },
    Migration {
        id: 12,
        name: "message_queued",
        sql: include_str!("0012_message_queued.sql"),
    },
    Migration {
        id: 13,
        name: "thread_archived_at",
        sql: include_str!("0013_thread_archived_at.sql"),
    },
    Migration {
        id: 14,
        name: "goal_thread_titles",
        sql: include_str!("0014_goal_thread_titles.sql"),
    },
    Migration {
        id: 15,
        name: "parts_cut_off_parent_plan_mode",
        sql: include_str!("0015_parts_cut_off_parent_plan_mode.sql"),
    },
    Migration {
        id: 16,
        name: "slash_commands",
        sql: include_str!("0016_slash_commands.sql"),
    },
    Migration {
        id: 17,
        name: "imported_threads",
        sql: include_str!("0017_imported_threads.sql"),
    },
    Migration {
        id: 18,
        name: "message_refusal",
        sql: include_str!("0018_message_refusal.sql"),
    },
];

fn fail(m: &Migration, e: impl std::fmt::Display) -> String {
    format!(
        "Database error: migration {:04}_{} failed: {e}",
        m.id, m.name
    )
}

fn db(e: impl std::fmt::Display) -> String {
    format!("Database error: {e}")
}

/// Brings the schema up to date. Returns how many migrations had run before, so 0 means the
/// database is new.
pub(super) fn run(conn: &mut Connection) -> Res<usize> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
             id INTEGER PRIMARY KEY,
             name TEXT NOT NULL,
             applied_at INTEGER NOT NULL
         );",
    )
    .map_err(db)?;
    record_earlier_builds(conn)?;
    let recorded = recorded(conn)?;
    check_history(&recorded)?;
    let from = recorded.len();
    for m in &MIGRATIONS[from..] {
        let tx = conn.transaction().map_err(db)?;
        tx.execute_batch(m.sql).map_err(|e| fail(m, e))?;
        tx.execute(
            "INSERT INTO schema_migrations (id, name, applied_at) VALUES (?1, ?2, unixepoch())",
            params![m.id, m.name],
        )
        .map_err(|e| fail(m, e))?;
        tx.pragma_update(None, "user_version", m.id)
            .map_err(|e| fail(m, e))?;
        tx.commit().map_err(|e| fail(m, e))?;
    }
    Ok(from)
}

/// A database made before `schema_migrations` existed only has `user_version`. Its first
/// `user_version` migrations are written into the table, under the names they have now.
fn record_earlier_builds(conn: &mut Connection) -> Res<()> {
    let recorded: usize = conn
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
        .map_err(db)?;
    let version: usize = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db)?;
    if recorded > 0 || version == 0 {
        return Ok(());
    }
    let Some(earlier) = MIGRATIONS.get(..version) else {
        return Err(newer_build(version));
    };
    let tx = conn.transaction().map_err(db)?;
    for m in earlier {
        tx.execute(
            "INSERT INTO schema_migrations (id, name, applied_at) VALUES (?1, ?2, unixepoch())",
            params![m.id, m.name],
        )
        .map_err(db)?;
    }
    tx.commit().map_err(db)
}

fn recorded(conn: &Connection) -> Res<Vec<(u32, String)>> {
    let mut stmt = conn
        .prepare("SELECT id, name FROM schema_migrations ORDER BY id")
        .map_err(db)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db)?;
    rows.collect::<Result<_, _>>().map_err(db)
}

/// The recorded history must be the start of `MIGRATIONS`, in order. Anything else was written
/// by another build; carrying on could leave the schema half-applied.
fn check_history(recorded: &[(u32, String)]) -> Res<()> {
    for (i, (id, name)) in recorded.iter().enumerate() {
        let Some(known) = MIGRATIONS.get(i) else {
            return Err(newer_build(i + 1));
        };
        if known.id != *id || known.name != name {
            return Err(format!(
                "Database error: the database's migration {id} is \"{name}\", but this version \
                 has \"{}\" there. It was probably written by a different build.",
                known.name
            ));
        }
    }
    Ok(())
}

fn newer_build(version: usize) -> String {
    format!(
        "Database error: the database is at schema version {version}, newer than this version \
         of Patchwork understands ({}). Update Patchwork to open it.",
        MIGRATIONS.len()
    )
}
