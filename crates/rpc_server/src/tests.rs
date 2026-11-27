//! End-to-end tests on real loopback sockets over an ephemeral port with [`MemoryPairingStore`].

// Rust guideline compliant 2026-10-09

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use patchwork_platform::Error;
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

use crate::{Backend, MemoryPairingStore, Options, PairingStore, Server, Touched, now_ms, qr_modules};

/// Minimal stand-in backend: one listed chat and a send that always works.
struct App {
    store: MemoryPairingStore,
}

impl Backend for App {
    fn pairing_store(&self) -> &dyn PairingStore {
        &self.store
    }

    fn call(self: Arc<Self>, method: &str, _params: &Value) -> Result<Value, Error> {
        match method {
            "chats.list" => Ok(json!([{ "id": "chat-1" }])),
            "turn.send" => Ok(json!({ "messageId": "m-1", "steered": false, "queued": false })),
            _ => Err(format!("{method} is not available in this version of Patchwork").into()),
        }
    }

    fn pushes(&self, touched: &Touched) -> Vec<(&'static str, Value)> {
        vec![("chat.upserted", json!({ "id": touched.chat_id }))]
    }
}

/// A running server and what reaches it.
struct Running {
    app: Arc<App>,
    server: Server,
    port: u16,
    token: String,
    dir: Option<PathBuf>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn start(web_dir: Option<PathBuf>, dir: Option<PathBuf>) -> Running {
    let app = Arc::new(App {
        store: MemoryPairingStore::new(),
    });
    let backend: Weak<dyn Backend> = Arc::downgrade(&app) as Weak<App>;
    let server = Server::default();
    let port = server
        .start(
            backend,
            Options {
                port: 0,
                network: false,
                web_dir,
            },
        )
        .expect("server starts on an ephemeral port");
    let token = server.info(false, true).token;
    Running {
        app,
        server,
        port,
        token,
        dir,
    }
}

fn temp_dir() -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("patchwork-server-{}", patchwork_platform::new_id()));
    std::fs::create_dir_all(&dir).expect("temp dir created");
    dir
}

fn running() -> Running {
    start(None, None)
}

/// Sends `raw` and reads the whole answer: its status and body.
fn send(port: u16, raw: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("tcp connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set read timeout");
    stream.write_all(raw.as_bytes()).expect("write request");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("read response");
    let status = answer[9..12].parse().expect("status code");
    let body = answer
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (status, body)
}

/// One call over `POST /rpc`: its status, and the answer as JSON.
fn rpc(port: u16, token: Option<&str>, method: &str, params: &Value) -> (u16, Value) {
    let body = json!({ "id": 7, "method": method, "params": params }).to_string();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let (status, answer) = send(
        port,
        &format!(
            "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    );
    (status, serde_json::from_str(&answer).expect("json body"))
}

fn socket(port: u16, token: &str) -> WebSocket<TcpStream> {
    let stream = TcpStream::connect(("127.0.0.1", port)).expect("tcp connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set read timeout");
    let (ws, _) = tungstenite::client(format!("ws://127.0.0.1:{port}/ws?token={token}"), stream)
        .expect("ws handshake");
    ws
}

/// Reads until the server closes the socket, and returns the close code.
fn close_code(ws: &mut WebSocket<TcpStream>) -> Option<u16> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match ws.read() {
            Ok(Message::Close(frame)) => return frame.map(|f| u16::from(f.code)),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

/// Pairs a device with `scopes`: its id and token.
fn pair(server: &Running, scopes: &[&str]) -> (String, String) {
    let (_, started) = rpc(
        server.port,
        Some(&server.token),
        "auth.pair.start",
        &json!({ "scopes": scopes }),
    );
    let code = started["result"]["code"]
        .as_str()
        .expect("code string")
        .to_string();
    // Typed the way a person might.
    let typed = code.replace('-', " ").to_lowercase();
    let (status, claimed) = rpc(
        server.port,
        None,
        "auth.pair.claim",
        &json!({ "code": typed, "name": "Phone", "platform": "ios" }),
    );
    assert_eq!(status, 200, "{claimed}");
    let result = &claimed["result"];
    (
        result["device"]["id"]
            .as_str()
            .expect("device id")
            .to_string(),
        result["token"].as_str().expect("token").to_string(),
    )
}

#[test]
fn a_wrong_or_missing_token_is_refused_and_the_app_token_works() {
    let server = running();
    let (status, answer) = rpc(server.port, None, "chats.list", &json!({}));
    assert_eq!(status, 401);
    assert_eq!(answer["error"]["message"], "Unauthorized");
    let (status, _) = rpc(server.port, Some("not-the-token"), "chats.list", &json!({}));
    assert_eq!(status, 401);

    let (status, answer) = rpc(server.port, Some(&server.token), "chats.list", &json!({}));
    assert_eq!(status, 200);
    assert_eq!(answer["id"], 7);
    assert_eq!(answer["result"][0]["id"], "chat-1");

    let (status, answer) = send(server.port, "GET /health HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(status, 200);
    assert!(answer.contains("\"ok\":true"), "{answer}");

    let mut ws = socket(server.port, "not-the-token");
    assert_eq!(close_code(&mut ws), Some(4401));
}

#[test]
fn a_claimed_code_gives_a_token_for_its_scopes_only() {
    let server = running();
    let (_, token) = pair(&server, &["read"]);
    let (status, answer) = rpc(server.port, Some(&token), "chats.list", &json!({}));
    assert_eq!(
        (status, &answer["result"][0]["id"]),
        (200, &json!("chat-1"))
    );

    let (_, answer) = rpc(
        server.port,
        Some(&token),
        "turn.send",
        &json!({ "chatId": "chat-1", "text": "hi" }),
    );
    assert_eq!(
        answer["error"]["message"],
        "This device's token may not call turn.send"
    );
    let (_, answer) = rpc(server.port, Some(&token), "auth.devices", &json!({}));
    assert!(answer.get("error").is_some(), "{answer}");

    let (_, writer) = pair(&server, &[]);
    let (_, answer) = rpc(
        server.port,
        Some(&writer),
        "turn.send",
        &json!({ "chatId": "chat-1", "text": "hi" }),
    );
    assert_eq!(answer["result"]["messageId"], "m-1");

    // Only a hash of each token is stored.
    let devices = server
        .app
        .store
        .paired_devices()
        .expect("list paired devices");
    assert_eq!(devices.len(), 2);
    assert!(
        devices
            .iter()
            .all(|d| token.starts_with(&d.token_hint) || writer.starts_with(&d.token_hint))
    );
}

#[test]
fn revoking_a_device_refuses_its_token_and_closes_its_socket() {
    let server = running();
    let (id, token) = pair(&server, &["read", "write"]);
    let mut ws = socket(server.port, &token);
    ws.send(Message::text(
        json!({ "id": 1, "method": "chats.list", "params": {} }).to_string(),
    ))
    .expect("send ws rpc");
    let reply: Value = serde_json::from_str(
        &ws.read()
            .expect("read ws reply")
            .into_text()
            .expect("text frame"),
    )
    .expect("json reply");
    assert_eq!(reply["result"][0]["id"], "chat-1");

    let (_, answer) = rpc(
        server.port,
        Some(&server.token),
        "auth.devices.revoke",
        &json!({ "id": id }),
    );
    assert_eq!(answer["result"], Value::Null, "{answer}");
    assert_eq!(close_code(&mut ws), Some(4401));
    let (status, _) = rpc(server.port, Some(&token), "chats.list", &json!({}));
    assert_eq!(status, 401);
}

#[test]
fn a_change_is_pushed_to_open_sockets() {
    let server = running();
    let mut ws = socket(server.port, &server.token);
    // The socket is registered once the server has read its token.
    ws.send(Message::text(
        json!({ "id": 1, "method": "chats.list" }).to_string(),
    ))
    .expect("send ws rpc");
    let _ = ws.read().expect("read ws reply");
    server.server.touch(Touched {
        chat_id: "chat-1".into(),
        ..Touched::default()
    });
    let push: Value = serde_json::from_str(
        &ws.read()
            .expect("read push frame")
            .into_text()
            .expect("text frame"),
    )
    .expect("json push");
    assert_eq!(push["push"], "chat.upserted");
    assert_eq!(push["data"]["id"], "chat-1");
    assert_eq!(push["seq"], 1);
}

#[test]
fn a_used_or_expired_code_is_refused() {
    let server = running();
    let (_, started) = rpc(
        server.port,
        Some(&server.token),
        "auth.pair.start",
        &json!({}),
    );
    let code = started["result"]["code"]
        .as_str()
        .expect("code string")
        .to_string();
    let claim = json!({ "code": code, "name": "Phone", "platform": "ios" });
    let (_, first) = rpc(server.port, None, "auth.pair.claim", &claim);
    assert!(first["result"]["token"].is_string(), "{first}");
    let (_, second) = rpc(server.port, None, "auth.pair.claim", &claim);
    assert_eq!(
        second["error"]["message"],
        "That code is wrong or has expired"
    );

    let now = now_ms();
    server
        .app
        .store
        .add_pair_code(
            "BCD-FGH",
            &["read".to_string()],
            None,
            now - 1,
            now - 60_000,
        )
        .expect("insert expired pair code");
    let (_, expired) = rpc(
        server.port,
        None,
        "auth.pair.claim",
        &json!({ "code": "BCD-FGH", "name": "Phone", "platform": "ios" }),
    );
    assert_eq!(
        expired["error"]["message"],
        "That code is wrong or has expired"
    );
}

#[test]
fn paths_outside_the_web_folder_are_refused() {
    let dir = temp_dir();
    let web = dir.join("web");
    std::fs::create_dir_all(&web).expect("create web dir");
    std::fs::write(web.join("index.html"), "<p>app</p>").expect("write index");
    std::fs::write(dir.join("secret.txt"), "secret").expect("write secret");
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.join("secret.txt"), web.join("link.txt"))
        .expect("create symlink");
    let server = start(Some(web), Some(dir));
    let get = |path: &str| {
        send(
            server.port,
            &format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n"),
        )
    };

    assert_eq!(get("/index.html"), (200, "<p>app</p>".to_string()));
    // A route of the web app gets the app.
    assert_eq!(get("/chats/1").1, "<p>app</p>");
    for path in [
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/a/%2E%2E/%2e%2e/secret.txt",
        "/..%5csecret.txt",
    ] {
        let (status, body) = get(path);
        assert_eq!(status, 403, "{path}");
        assert!(!body.contains("secret"), "{path}");
    }
    #[cfg(unix)]
    assert_eq!(get("/link.txt").0, 403);
}

#[test]
fn qr_codes_are_square_and_refuse_text_too_long() {
    let modules = qr_modules("HELLO WORLD").expect("qr modules");
    assert_eq!(modules.len(), 21);
    assert!(modules.iter().all(|row| row.len() == 21));
    let link = qr_modules("http://192.168.1.5:7703/pair?code=K7M-QX2").expect("pair link qr");
    assert_eq!(link.len() % 4, 1);
    assert!(link.iter().all(|row| row.len() == link.len()));
    assert!(qr_modules(&"a".repeat(5000)).is_none());
}

/// One call over `POST /rpc`, answered with the bytes sent.
fn rpc_bytes(port: u16, token: Option<&str>, method: &str, params: &Value) -> String {
    let body = json!({ "id": 7, "method": method, "params": params }).to_string();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    send(
        port,
        &format!(
            "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
    .1
}

#[test]
fn server_answer_bytes_are_pinned() {
    let server = running();
    let port = server.port;
    let admin = Some(server.token.as_str());

    let started = rpc_bytes(
        port,
        admin,
        "auth.pair.start",
        &json!({ "scopes": ["read"] }),
    );
    let value: Value = serde_json::from_str(&started).expect("json");
    let code = value["result"]["code"]
        .as_str()
        .expect("code string")
        .to_string();
    assert_eq!(
        started,
        format!(
            r#"{{"id":7,"result":{{"code":"{code}","expiresAt":{},"scopes":["read"]}}}}"#,
            value["result"]["expiresAt"]
        )
    );

    let claimed = rpc_bytes(
        port,
        None,
        "auth.pair.claim",
        &json!({ "code": code, "name": "Phone", "platform": "ios" }),
    );
    let value: Value = serde_json::from_str(&claimed).expect("json");
    let device = &value["result"]["device"];
    let device_bytes = format!(
        r#"{{"id":{},"name":"Phone","platform":"ios","scopes":["read"],"createdAt":{},"lastSeenAt":{},"tokenHint":{}}}"#,
        device["id"], device["createdAt"], device["lastSeenAt"], device["tokenHint"]
    );
    assert_eq!(
        claimed,
        format!(
            r#"{{"id":7,"result":{{"token":{},"device":{device_bytes}}}}}"#,
            value["result"]["token"]
        )
    );

    let devices = rpc_bytes(port, admin, "auth.devices", &json!({}));
    let value: Value = serde_json::from_str(&devices).expect("json");
    let device = &value["result"][0];
    assert_eq!(
        devices,
        format!(
            r#"{{"id":7,"result":[{{"id":{},"name":"Phone","platform":"ios","scopes":["read"],"createdAt":{},"lastSeenAt":{},"tokenHint":{}}}]}}"#,
            device["id"], device["createdAt"], device["lastSeenAt"], device["tokenHint"]
        )
    );

    assert_eq!(
        rpc_bytes(port, admin, "pairing.info", &json!({})),
        format!(
            r#"{{"id":7,"result":{{"urls":[],"token":"{}","reachable":false}}}}"#,
            server.token
        )
    );
    assert_eq!(
        rpc_bytes(port, admin, "events.since", &json!({})),
        r#"{"id":7,"result":{"floor":1,"next":1,"events":[]}}"#
    );
    assert_eq!(
        rpc_bytes(port, admin, "nothing.here", &json!({})),
        r#"{"id":null,"error":{"message":"nothing.here is not available in this version of Patchwork"}}"#
    );
    assert_eq!(
        rpc_bytes(port, None, "chats.list", &json!({})),
        r#"{"error":{"message":"Unauthorized"}}"#
    );
    assert_eq!(
        send(port, "GET /health HTTP/1.1\r\nHost: x\r\n\r\n").1,
        format!(
            r#"{{"ok":true,"version":"{}","seq":1}}"#,
            patchwork_platform::VERSION
        )
    );

    let mut ws = socket(port, &server.token);
    ws.send(Message::text(
        json!({ "id": 1, "method": "chats.list" }).to_string(),
    ))
    .expect("send ws rpc");
    assert_eq!(
        ws.read()
            .expect("read reply")
            .into_text()
            .expect("text")
            .as_str(),
        r#"{"id":1,"result":[{"id":"chat-1"}]}"#
    );
    server.server.touch(Touched {
        chat_id: "chat-1".into(),
        ..Touched::default()
    });
    assert_eq!(
        ws.read()
            .expect("read push")
            .into_text()
            .expect("text")
            .as_str(),
        r#"{"push":"chat.upserted","data":{"id":"chat-1"},"seq":1}"#
    );
}
