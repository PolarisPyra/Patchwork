# Layers

Patchwork's apps are built in layers, after the layering of Discord's Rust client. Each layer
depends only on the ones above it in this list.

1. **Model and protocol** (`crates/patchwork_model`, `crates/patchwork_protocol`): plain domain
   types (chats, messages, agents, providers, settings values) and the typed methods, pushes and
   frames a client and the server exchange. Plain serde types, no GPUI, no I/O.
2. **Engine** (`crates/patchwork_core` and the crates beside it): chats, agents, providers and
   storage, with no UI. `patchwork_harness` (Patchwork's own agent and its hooks and relays) and
   `patchwork_sidecars` (Android, the browser, the terminal, the computer) sit under the core;
   `patchwork_server` (the sockets phones and browsers use) and `patchwork_gateway` (the chat apps)
   sit over it.
3. **Client** (`crates/patchwork_client`): the state screens read, with no drawing.
   - `Session` is a GPUI global kept from protocol pushes: pairing, the chat list, the open chat,
     pending approvals. A `Transport` feeds it; the phone uses `RemoteTransport` over a websocket.
   - With the `local` feature, `AppState` and `GitModel` hold the desktop's richer view of an
     in-process `Core`, and `local::core` re-exports the core for the screens. The phone builds
     without `local` and never links the engine.
4. **Design and views**: drawing that knows nothing of a window.
   - **Design** (`crates/design/*`, `patchwork-design-*`): icons, theme, tasks, components, text,
     editor and backgrounds. Domain-free: no design crate depends on `patchwork-model` or below.
   - **Views** (`crates/views/*`, `patchwork-views-*`): logos, chat and actions, drawn from
     Patchwork's data. A view crate may depend on `patchwork-model` and the design crates, never on
     the client or the core.
5. **Features** (`crates/features/*`): one crate per screen area. Desktop areas are
   `desktop_<area>` (`patchwork-desktop-<area>`); the phone's screens share one crate, `mobile`
   (`patchwork-mobile`). A feature is a view entity that holds its own UI state and reads the
   client. A feature never depends on another feature, nor on the core directly: it reaches the
   core through `patchwork_client::local::core`.
6. **Shell** (`apps/<app>/src`): the window or activity, its navigation, the panels the features
   sit in, and the wiring that makes one feature answer another's events.

## Client

- State is a plain struct reached through a GPUI global (`AppState::get(cx)`, `Session::get(cx)`),
  changed through its `pub fn`s. The app has one window, as both assume.
- Client state holds data: ids, chats, messages, git status. It holds no `FocusHandle`,
  `ScrollHandle`, `ListState`, element, or which menu is open. Those belong to the screen.
- Work that blocks runs off the UI thread; the result is applied back on it.

## Features

- A feature takes an `Rc<dyn …Host>` for what only the shell can give, and emits an event enum
  (`SidebarEvent`, `ChatEvent`, `ChatsEvent`, …) for what it asks the shell to do.
- A feature never names another feature. The shell's `wiring/<area>.rs` makes the view,
  implements its host, subscribes, and does what its events ask.
- The chat is the one feature drawn by the window itself. `desktop_chat` holds its state in
  `ChatUi` (the transcript's view state, the panes and tabs, the strips above the composer) and
  asks the window for the rest through the `ChatWindow` trait. The shell's `Workspace` holds a
  `ChatUi`, derefs to it, and implements `ChatWindow`. The chat's code is written as extension
  traits over any `ChatWindow`, brought in by `patchwork_desktop_chat::prelude`.
- A view crate carries `#![expect(clippy::unused_self, reason = ...)]`, since a view acts
  through its `Context<Self>`.

## Shell

- Desktop: `window.rs` holds the `Workspace`; `nav/` the page, place and history; `panels/` the
  frame and its panels (`panels.home`, `panels.chat`, `panels.details`, `panels.dock`); `wiring/`
  one file per feature crate.
- Android: `app.rs` loads the fonts and theme from the design crates; `nav.rs` switches between the
  `mobile` crate's `chats`, `chat`, `approvals` and `pairing` screens.

## Element ids

Every element a person can point at has a stable id: `<area>.<part>`, then `:<key>` for one of
many. For example `panels.chat`, `composer.send`, `message.content:<id>`, `sidebar.chat:<id>`,
`settings.row.<key>`.
