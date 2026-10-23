//! App state persisted in SQLite (`patchwork.db` in the app config dir).
//!
//! - `store`: the `Store` the rest of the app uses, with its in-memory cache.
//! - `connection`: opening the file, then migrating it.
//! - `migrations`: the schema, as numbered SQL files, and the record of which have run.
//! - `settings`, `threads`: how each table's rows map to the app's types.
//! - `codec`: how values are written to columns as text.
//! - `legacy`: data from before the database, or from before it moved.

mod codec;
mod connection;
mod legacy;
mod migrations;
mod settings;
mod store;
mod threads;

pub use legacy::adopt_old_dir;
pub use store::Store;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::connection::DB_FILE;
    use super::*;
    use crate::types::{AgentKind, Message, MessageStatus, Role, Thread, new_id};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(new_id());
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn message(text: &str, status: MessageStatus) -> Message {
        Message {
            id: new_id(),
            role: Role::Assistant,
            text: text.into(),
            created_at: 1,
            status,
            tools: vec![],
            plan: vec![],
            files: vec!["a.rs".into()],
            error: None,
            approvals: vec![],
            attachments: vec![],
            steer: false,
            queued: false,
            background: Vec::new(),
            cut_off: false,
            refusal: None,
            parts: Vec::new(),
            finished_at: None,
            usage: None,
        }
    }

    fn thread(messages: Vec<Message>) -> Thread {
        Thread {
            id: new_id(),
            project_id: None,
            title: "t".into(),
            agent: AgentKind::Antigravity,
            model: Some("pro".into()),
            session_id: Some("s".into()),
            access_mode: crate::types::AccessMode::Supervised,
            created_at: 2,
            messages,
            goal: None,
            position: 0,
            folder_id: None,
            effort: None,
            archived_at: None,
            parent: None,
            plan_mode: false,
            slash_commands: vec![],
            imported_from: None,
            context_pending: false,
        }
    }

    #[test]
    fn old_dir_is_adopted_once() {
        let (old, new) = (temp_dir(), temp_dir().join("patchwork"));
        let store = Store::open(old.clone()).unwrap();
        store.update_settings(|s| s.onboarded = true).unwrap();
        drop(store);

        adopt_old_dir(&old, &new).unwrap();
        assert!(!old.exists());
        assert!(Store::open(new.clone()).unwrap().settings().onboarded);

        // A database already in the new folder wins.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join(DB_FILE), "stale").unwrap();
        adopt_old_dir(&old, &new).unwrap();
        assert!(old.join(DB_FILE).exists());
        let _ = std::fs::remove_dir_all(&old);
        let _ = std::fs::remove_dir_all(new.parent().unwrap());
    }

    #[test]
    fn legacy_json_moves_into_sqlite() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"onboarded":true,"agent":"codex","projects":[{"id":"p","name":"n","path":"/x"}]}"#,
        )
        .unwrap();
        let t = thread(vec![message("hi", MessageStatus::Streaming)]);
        std::fs::write(
            dir.join("threads.json"),
            serde_json::to_string(&[&t]).unwrap(),
        )
        .unwrap();

        let store = Store::open(dir.clone()).unwrap();
        let s = store.settings();
        assert_eq!(s.agents, [AgentKind::Codex]);
        assert_eq!(s.projects[0].path, "/x");
        let loaded = store.thread(&t.id).unwrap();
        assert_eq!(loaded.messages[0].status, MessageStatus::Cancelled);
        assert_eq!(loaded.messages[0].files, ["a.rs"]);
        assert!(dir.join("settings.json.bak").exists() && !dir.join("threads.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thread_changes_survive_reopen_and_delete_cascades() {
        let dir = temp_dir();
        let store = Store::open(dir.clone()).unwrap();
        let t = thread(vec![message("a", MessageStatus::Complete)]);
        store.insert_threads(vec![t.clone()]).unwrap();
        store
            .update_thread(&t.id, |t| {
                t.title = "renamed".into();
                t.slash_commands = vec![crate::types::ProviderSlashCommand {
                    name: "review".into(),
                    description: Some("Review the current changes".into()),
                }];
                t.messages[0].text.push('b');
                t.messages.push(message("c", MessageStatus::Complete));
            })
            .unwrap();
        drop(store);

        let store = Store::open(dir.clone()).unwrap();
        let loaded = store.thread(&t.id).unwrap();
        assert_eq!(loaded.title, "renamed");
        assert_eq!(loaded.slash_commands[0].name, "review");
        assert_eq!(loaded.slash_commands[0].description.as_deref(), Some("Review the current changes"));
        let texts: Vec<_> = loaded.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["ab", "c"]);
        assert_eq!(store.session_keys(), [(AgentKind::Antigravity, "s".to_string())]);

        store.delete_thread(&t.id).unwrap();
        let count: i64 = store
            .lock()
            .conn
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
