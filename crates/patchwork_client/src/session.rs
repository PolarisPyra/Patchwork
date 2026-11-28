//! The client store: a Patchwork's chats, the open chat's messages, the approvals waiting, and
//! how the connection stands, kept current from a [`Transport`].

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui::{App, AppContext as _, BorrowAppContext as _, Global};
use patchwork_protocol::Method;
use patchwork_protocol::NoParams;
use patchwork_protocol::chats::ChatParams;
use patchwork_protocol::methods::{AnswerApproval, ListChats, ListMessages, SendTurn, StopTurn};
use patchwork_protocol::pushes::Push;
use patchwork_protocol::turns::{AnswerParams, SendParams};

use crate::stores::{
    ApprovalsStore, ChatsStore, ConnectionState, MessagesStore, Pairing, PendingApproval,
};
use crate::transport::{ClientError, RemoteTransport, Transport};

/// A Patchwork as a client sees it, reached through a [`Transport`]. A GPUI global: screens
/// observe it with `cx.observe_global::<Session>`.
#[derive(Debug, Default)]
pub struct Session {
    /// The transport, how pairing stands, and the last call that failed.
    pub connection: ConnectionState,
    /// Every chat, and which one is open.
    pub chats: ChatsStore,
    /// The open chat's messages.
    pub messages: MessagesStore,
    /// The approvals the agents wait on.
    pub approvals: ApprovalsStore,
}

impl Global for Session {}

impl Session {
    /// Sets up an empty session; call once at startup, before any screen reads it.
    pub fn init(cx: &mut App) {
        cx.set_global(Self::default());
    }

    /// The session.
    #[must_use]
    pub fn get(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// Takes answers and pushes from `transport`, reached at `address`, and loads its chats.
    pub fn connect(transport: Arc<dyn Transport>, address: String, cx: &mut App) {
        let (pushes, mut received) = mpsc::unbounded::<Push>();
        transport.subscribe(Box::new(move |push| {
            let _ = pushes.unbounded_send(push);
        }));
        cx.spawn(async move |cx| {
            while let Some(push) = received.next().await {
                cx.update(|cx| cx.update_global::<Self, _>(|session, _| session.apply(push)));
            }
        })
        .detach();
        cx.update_global::<Self, _>(|session, _| {
            session.connection.transport = Some(transport);
            session.connection.pairing = Pairing::Connected(address);
            session.connection.error = None;
        });
        Self::reload_chats(cx);
    }

    /// Claims pairing `code` at `address` as the device `name`, then connects with the token it
    /// gets. The token is not kept: a restart pairs again.
    pub fn pair(address: String, code: String, name: String, cx: &mut App) {
        cx.update_global::<Self, _>(|session, _| {
            session.connection.pairing = Pairing::Connecting;
        });
        let task = cx.background_spawn({
            let address = address.clone();
            async move {
                let claimed = RemoteTransport::pair(&address, &code, &name, std::env::consts::OS)?;
                RemoteTransport::connect(&address, &claimed.token)
            }
        });
        cx.spawn(async move |cx| {
            let connected = task.await;
            cx.update(|cx| match connected {
                Ok(transport) => Self::connect(Arc::new(transport), address, cx),
                Err(e) => cx.update_global::<Self, _>(|session, _| {
                    session.connection.pairing = Pairing::Failed(e.to_string());
                }),
            });
        })
        .detach();
    }

    /// Loads the chat list anew.
    pub fn reload_chats(cx: &mut App) {
        Self::call::<ListChats>(NoParams {}, cx, |session, chats| session.chats.list = chats);
    }

    /// Opens chat `chat_id` and loads its messages.
    pub fn open(chat_id: String, cx: &mut App) {
        cx.update_global::<Self, _>(|session, _| {
            session.chats.open = Some(chat_id.clone());
            session.messages.clear();
        });
        let params = ChatParams {
            chat_id: chat_id.clone(),
        };
        Self::call::<ListMessages>(params, cx, move |session, messages| {
            if session.chats.is_open(&chat_id) {
                for message in &messages {
                    session.approvals.note(message);
                }
                session.messages.list = messages;
            }
        });
    }

    /// Closes the open chat.
    pub fn close(cx: &mut App) {
        cx.update_global::<Self, _>(|session, _| {
            session.chats.open = None;
            session.messages.clear();
        });
    }

    /// Sends `text` to the open chat.
    pub fn send(text: String, cx: &mut App) {
        let Some(chat_id) = Self::get(cx).chats.open.clone() else {
            return;
        };
        Self::call::<SendTurn>(SendParams { chat_id, text }, cx, |_, _| {});
    }

    /// Stops the open chat's turn.
    pub fn stop(cx: &mut App) {
        let Some(chat_id) = Self::get(cx).chats.open.clone() else {
            return;
        };
        Self::call::<StopTurn>(ChatParams { chat_id }, cx, |_, ()| {});
    }

    /// Allows or denies a waiting approval.
    pub fn answer(approval: &PendingApproval, allow: bool, cx: &mut App) {
        let params = AnswerParams {
            chat_id: approval.chat_id.clone(),
            approval_id: approval.id.clone(),
            allow: Some(allow),
            answers: None,
        };
        let id = approval.id.clone();
        Self::call::<AnswerApproval>(params, cx, move |session, ()| {
            session.approvals.remove(&id);
        });
    }

    /// Calls `M` off the UI thread and hands its answer to `done`, or keeps its error.
    fn call<M: Method>(
        params: M::Params,
        cx: &mut App,
        done: impl FnOnce(&mut Self, M::Output) + 'static,
    ) where
        M::Params: Send + 'static,
        M::Output: Send + 'static,
    {
        let Some(transport) = Self::get(cx).connection.transport.clone() else {
            return;
        };
        let task = cx.background_spawn(async move { transport.call::<M>(&params) });
        cx.spawn(async move |cx| {
            let answer: Result<M::Output, ClientError> = task.await;
            cx.update(|cx| {
                cx.update_global::<Self, _>(|session, _| match answer {
                    Ok(output) => done(session, output),
                    Err(e) => session.connection.error = Some(e.to_string()),
                });
            });
        })
        .detach();
    }

    /// Takes in one push, handing each part to its store.
    fn apply(&mut self, push: Push) {
        match push {
            Push::ChatUpserted(chat) => self.chats.upsert(chat),
            Push::ChatDeleted(gone) => {
                self.approvals.forget_chat(&gone.id);
                if self.chats.remove(&gone.id) {
                    self.messages.clear();
                }
            }
            Push::MessageUpserted(message) => {
                self.approvals.note(&message);
                if self.chats.is_open(&message.chat_id) {
                    self.messages.upsert(message);
                }
            }
            Push::MessageDeleted(gone) => {
                self.messages.remove(&gone.id);
                self.approvals.forget_message(&gone.id);
            }
        }
    }
}
