//! The app's actions, which the keys are bound to and the menus and pages send, and the
//! window's title.

// Rust guideline compliant 2026-10-09

use gpui::actions;

actions!(
    patchwork,
    [
        FocusNext,
        FocusPrev,
        Quit,
        TogglePalette,
        NewChat,
        FindInChat,
        CloseChatTab,
        OpenSettings,
        ShowOrchestrator,
        ShowTasks,
        ShowChanges,
        ShowQuilts,
        ShowArchived,
        ToggleTerminal,
        NewTerminal,
        TogglePanel,
        ToggleOutput,
        DeleteChat,
        GoBack,
        GoForward,
        SplitPaneRight,
        SplitPaneDown,
        FocusPaneLeft,
        FocusPaneRight,
        FocusPaneUp,
        FocusPaneDown,
        FocusPane1,
        FocusPane2,
        FocusPane3,
        FocusPane4,
        NextTab,
        PrevTab,
        ReopenTab,
        MoveTabLeft,
        MoveTabRight,
        MoveTabUp,
        MoveTabDown,
        SwapPaneLeft,
        SwapPaneRight,
        SwapPaneUp,
        SwapPaneDown,
        MaximizePane
    ]
);

/// The window's title, marked on debug builds so they are not mistaken for the installed app.
pub const APP_TITLE: &str = if cfg!(debug_assertions) {
    "Patchwork (dev)"
} else {
    "Patchwork"
};
