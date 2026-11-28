//! Which screen the phone shows, and the moves between them that the screens' events ask for.

// Rust guideline compliant 2026-11-27

use gpui::{
    AnyView, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Window, div,
};
use patchwork_client::Session;
use patchwork_mobile::{
    ApprovalsEvent, ApprovalsScreen, ChatEvent, ChatScreen, ChatsEvent, ChatsScreen, PairingEvent,
    PairingScreen,
};
use patchwork_design_theme::{fs_md, theme};

/// A screen the phone can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Pairing,
    Chats,
    Chat,
    Approvals,
}

/// The app's root view: every screen, and the one shown.
#[derive(Debug)]
pub struct Nav {
    shown: Screen,
    pairing: Entity<PairingScreen>,
    chats: Entity<ChatsScreen>,
    chat: Entity<ChatScreen>,
    approvals: Entity<ApprovalsScreen>,
}

impl Nav {
    /// Makes the screens and answers their events. Starts on pairing.
    #[must_use]
    pub fn new(cx: &mut Context<'_, Self>) -> Self {
        let pairing = cx.new(PairingScreen::new);
        let chats = cx.new(ChatsScreen::new);
        let chat = cx.new(ChatScreen::new);
        let approvals = cx.new(ApprovalsScreen::new);
        cx.subscribe(&pairing, |nav, _, event, cx| match event {
            PairingEvent::Paired(_) => nav.show(Screen::Chats, cx),
        })
        .detach();
        cx.subscribe(&chats, |nav, _, event, cx| match event {
            ChatsEvent::Open(chat_id) => {
                Session::open(chat_id.clone(), cx);
                nav.show(Screen::Chat, cx);
            }
            ChatsEvent::ShowApprovals => nav.show(Screen::Approvals, cx),
        })
        .detach();
        cx.subscribe(&chat, |nav, _, event, cx| match event {
            ChatEvent::Back => nav.show(Screen::Chats, cx),
        })
        .detach();
        cx.subscribe(&approvals, |nav, _, event, cx| match event {
            ApprovalsEvent::Back => nav.show(Screen::Chats, cx),
        })
        .detach();
        Self {
            shown: Screen::Pairing,
            pairing,
            chats,
            chat,
            approvals,
        }
    }

    /// Shows `screen`.
    fn show(&mut self, screen: Screen, cx: &mut Context<'_, Self>) {
        self.shown = screen;
        cx.notify();
    }
}

impl Render for Nav {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let t = theme(cx);
        let screen: AnyView = match self.shown {
            Screen::Pairing => self.pairing.clone().into(),
            Screen::Chats => self.chats.clone().into(),
            Screen::Chat => self.chat.clone().into(),
            Screen::Approvals => self.approvals.clone().into(),
        };
        div()
            .size_full()
            .bg(t.bg_app)
            .text_color(t.text)
            .text_size(gpui::px(fs_md()))
            .child(screen)
    }
}
