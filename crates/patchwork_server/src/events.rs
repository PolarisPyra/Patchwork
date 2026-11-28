//! What the server pushes to open sockets when a chat changes.

// Rust guideline compliant 2026-10-09

use patchwork_protocol::pushes::{ChatGone, MessageGone, Push};

use super::Touched;
use super::wire::{client_chat, client_message};
use patchwork_core::Core;
use patchwork_core::events::ChatEvent;

/// The pushes that bring a client up to date after the change `touched` describes.
pub(super) fn client_pushes(core: &Core, touched: &Touched) -> Vec<Push> {
    let deleted = || {
        vec![Push::ChatDeleted(ChatGone {
            id: touched.chat_id.clone(),
        })]
    };
    if touched.deleted {
        return deleted();
    }
    let Some(chat) = core.store().chat(&touched.chat_id) else {
        return deleted();
    };
    // A subagent's chat is not listed, so its changes are not news to a client.
    if chat.parent.is_some() {
        return Vec::new();
    }
    let mut pushes = vec![Push::ChatUpserted(client_chat(core, &chat))];
    pushes.extend(
        chat.messages
            .iter()
            .filter(|m| touched.messages.contains(&m.id))
            .map(|m| Push::MessageUpserted(client_message(&chat.id, m))),
    );
    pushes.extend(touched.removed.iter().map(|id| {
        Push::MessageDeleted(MessageGone {
            chat_id: chat.id.clone(),
            id: id.clone(),
        })
    }));
    pushes
}

/// What a chat event changed, for the pushes.
pub(super) fn touched_by(chat_id: &str, event: &ChatEvent) -> Touched {
    let mut touched = Touched {
        chat_id: chat_id.to_string(),
        ..Touched::default()
    };
    match event {
        ChatEvent::MessageRemoved { message_id } => touched.removed.push(message_id.clone()),
        ChatEvent::ChatDeleted => touched.deleted = true,
        // The rest change a message clients show, or else the chat, which is pushed anew.
        _ => touched
            .messages
            .extend(event.client_message_id().map(String::from)),
    }
    touched
}
