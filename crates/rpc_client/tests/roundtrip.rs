//! End-to-end integration tests between `patchwork-rpc-client` and `patchwork-rpc-server`.

// Rust guideline compliant 2026-10-09

use std::sync::{Arc, Weak};
use std::time::Duration;

use patchwork_rpc_client::{ClientError, RpcClient, WsClient};
use patchwork_rpc_protocol::chats::{AccessMode, Chat, ChatStatus};
use patchwork_rpc_protocol::methods::{ListChats, SendTurn};
use patchwork_rpc_protocol::turns::{SendParams, Sent};
use patchwork_rpc_server::{Backend, MemoryPairingStore, Options, PairingStore, Server, Touched};
use patchwork_platform::Error;
use serde_json::{Value, json};

struct TestBackend {
    store: MemoryPairingStore,
}

impl Backend for TestBackend {
    fn pairing_store(&self) -> &dyn PairingStore {
        &self.store
    }

    fn call(self: Arc<Self>, method: &str, params: &Value) -> Result<Value, Error> {
        match method {
            "chats.list" => {
                let chats = vec![Chat {
                    id: "chat-1".into(),
                    project_id: None,
                    title: "Hello".into(),
                    agent: "claude".into(),
                    model: None,
                    access_mode: AccessMode::FullAccess,
                    session_id: None,
                    effort: None,
                    status: ChatStatus::Idle,
                    position: 0,
                    created_at: 1,
                    updated_at: 2,
                    archived_at: None,
                    pinned: false,
                    worktree_path: None,
                    branch: None,
                    checkpoint_ref: None,
                }];
                Ok(serde_json::to_value(chats).expect("serialize chats"))
            }
            "turn.send" => {
                let parsed: SendParams = serde_json::from_value(params.clone())
                    .map_err(|e| format!("bad params: {e}"))?;
                assert_eq!(parsed.chat_id, "chat-1");
                let sent = Sent {
                    message_id: "msg-1".into(),
                    steered: false,
                    queued: false,
                };
                Ok(serde_json::to_value(sent).expect("serialize sent"))
            }
            _ => Err(format!("unknown method {method}").into()),
        }
    }

    fn pushes(&self, touched: &Touched) -> Vec<(&'static str, Value)> {
        vec![("chat.upserted", json!({ "id": touched.chat_id }))]
    }
}

fn start_server() -> (Arc<TestBackend>, Server, String, String) {
    let backend = Arc::new(TestBackend {
        store: MemoryPairingStore::new(),
    });
    let server = Server::default();
    let weak: Weak<dyn Backend> = Arc::downgrade(&backend) as Weak<TestBackend>;
    let port = server
        .start(
            weak,
            Options {
                port: 0,
                network: false,
                web_dir: None,
            },
        )
        .expect("server binds ephemeral port");
    let info = server.info(false, true);
    (
        backend,
        server,
        format!("http://127.0.0.1:{port}"),
        info.token,
    )
}

#[test]
fn http_and_pairing_roundtrip() {
    let (_backend, _server, base_url, admin_token) = start_server();

    // Unauthenticated health works; unauthenticated RPC is rejected with Unauthorized.
    let anon = RpcClient::new(&base_url, None).expect("client");
    let health = anon.health().expect("health");
    assert!(health.ok);
    assert!(matches!(
        anon.call_empty::<ListChats>(),
        Err(ClientError::Unauthorized(_))
    ));

    // Admin client can call typed methods and start pairing.
    let admin = RpcClient::new(&base_url, Some(admin_token)).expect("admin client");
    let chats = admin.call_empty::<ListChats>().expect("list chats");
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].id, "chat-1");

    let code = admin
        .pair_start(Some("Phone"), &["read"])
        .expect("pair start");
    let claimed = anon
        .pair_claim(&code.code, "Phone", "ios")
        .expect("pair claim");

    // Read-only device can list chats, cannot send turns.
    let readonly = RpcClient::new(&base_url, Some(claimed.token)).expect("readonly client");
    assert_eq!(
        readonly
            .call_empty::<ListChats>()
            .expect("list chats")
            .len(),
        1
    );
    let denied = readonly.call::<SendTurn>(&SendParams {
        chat_id: "chat-1".into(),
        text: "hello".into(),
    });
    assert!(matches!(denied, Err(ClientError::Rpc(_))));

    // Admin can send turns and list/revoke devices.
    let sent = admin
        .call::<SendTurn>(&SendParams {
            chat_id: "chat-1".into(),
            text: "hello".into(),
        })
        .expect("send turn");
    assert_eq!(sent.message_id, "msg-1");
    assert_eq!(admin.devices().expect("devices").len(), 1);
    admin
        .revoke_device(&claimed.device.id)
        .expect("revoke device");
    assert!(matches!(
        readonly.call_empty::<ListChats>(),
        Err(ClientError::Unauthorized(_))
    ));
}

#[test]
fn websocket_rpc_push_and_revocation() {
    let (_backend, server, base_url, admin_token) = start_server();
    let admin = RpcClient::new(&base_url, Some(admin_token)).expect("admin client");
    let anon = RpcClient::new(&base_url, None).expect("anon client");

    let code = admin
        .pair_start(Some("Tablet"), &["read", "write"])
        .expect("pair start");
    let claimed = anon
        .pair_claim(&code.code, "Tablet", "ios")
        .expect("pair claim");

    let mut ws = WsClient::connect(&base_url, &claimed.token).expect("ws connect");
    let chats = ws.call_empty::<ListChats>().expect("ws list chats");
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].id, "chat-1");

    assert!(
        ws.try_next_push(Duration::from_millis(40))
            .expect("poll timeout")
            .is_none()
    );

    server.touch(Touched {
        chat_id: "chat-1".into(),
        ..Touched::default()
    });
    let push = ws.next_push().expect("receive push");
    assert_eq!(push.push, "chat.upserted");
    assert_eq!(push.data["id"], "chat-1");
    assert_eq!(push.seq, 1);

    // Revoking the paired device closes its WebSocket with 4401 (Unauthorized).
    admin
        .revoke_device(&claimed.device.id)
        .expect("revoke device");
    let after_revoke = ws.next_push();
    assert!(
        matches!(after_revoke, Err(ClientError::Unauthorized(_))),
        "expected Unauthorized close, got {after_revoke:?}"
    );
}
