//! The Screen view (`ScreenView.tsx`): the screen as an agent with computer use sees it. It only
//! looks: every click and keystroke stays with the agent, approved in the chat.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use gpui::{
    AnyElement, Context, Image, ImageFormat, ObjectFit, Pixels, Role, Size, Task, Toggled, Window,
    canvas, div, img, prelude::*, px,
};

use super::Panel;
use crate::theme::{FS_SM, RADIUS_SM, theme};
use crate::ui::{BtnKind, btn, icon, spinner};

/// How long Live waits after a capture finishes before taking the next. A capture itself takes
/// about four seconds on Wayland, so a fixed timer would pile them up.
const LIVE_GAP: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct ScreenState {
    shot: Option<(Arc<Image>, u32, u32)>,
    error: Option<String>,
    busy: bool,
    pub live: bool,
    pub(super) capture: Option<Task<()>>,
    /// The room the picture has, which it shrinks to fit.
    area: Option<Size<Pixels>>,
}

impl Panel {
    /// Takes a picture of the screen, and with Live on, another a beat after it comes back.
    pub(super) fn capture(&mut self, cx: &mut Context<Self>) {
        if self.screen.busy {
            return;
        }
        self.screen.busy = true;
        let task = cx.background_spawn(async {
            patchwork_server::computer::ensure_enabled()?;
            let shot = patchwork_server::computer::screenshot()?;
            let data = shot
                .data_url
                .split_once(";base64,")
                .map(|(_, data)| data)
                .ok_or("The screenshot could not be read")?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| e.to_string())?;
            let image = Arc::new(Image::from_bytes(ImageFormat::Png, bytes));
            Ok::<_, String>((image, shot.screen_width, shot.screen_height))
        });
        self.screen.capture = Some(cx.spawn(async move |this, cx| {
            let shot = task.await;
            let Ok(live) = this.update(cx, |this, cx| {
                this.screen.busy = false;
                match shot {
                    Ok(shot) => {
                        this.screen.shot = Some(shot);
                        this.screen.error = None;
                    }
                    Err(e) => this.screen.error = Some(e),
                }
                cx.notify();
                this.screen.live
            }) else {
                return;
            };
            if live {
                cx.background_executor().timer(LIVE_GAP).await;
                let _ = this.update(cx, |this, cx| {
                    if this.screen.live && this.tab == super::PanelTab::Screen && this.visible {
                        this.screen.capture = None;
                        this.capture(cx);
                    }
                });
            }
        }));
        cx.notify();
    }

    pub(super) fn screen_view(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = *theme(cx);
        if !self.computer_use {
            return div()
                .flex()
                .flex_1()
                .h_full()
                .items_center()
                .justify_center()
                .px(px(12.))
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child("Computer use is off. Turn it on under Settings → Permissions.")
                .into_any_element();
        }
        let busy = self.screen.busy;
        let live = self.screen.live;
        let fg = t.text;
        // The picture is drawn no larger than it is, and smaller when it has to fit.
        let fit = match (&self.screen.shot, self.screen.area) {
            (Some((_, w, h)), Some(area)) => (f32::from(area.width) / *w as f32)
                .min(f32::from(area.height) / *h as f32)
                .min(1.),
            _ => 1.,
        };
        let status = match (&self.screen.error, &self.screen.shot) {
            (Some(e), _) => Some(
                div()
                    .id("screen-error")
                    .role(Role::Alert)
                    .min_w_0()
                    .truncate()
                    .text_color(t.danger)
                    .child(e.clone()),
            ),
            (None, Some((_, w, h))) => {
                let scale = (fit * 100.).round();
                Some(
                    div()
                        .id("screen-size")
                        .min_w_0()
                        .truncate()
                        .text_color(t.text_muted)
                        .child(if scale == 100. {
                            format!("{w}×{h}")
                        } else {
                            format!("{w}×{h} · shown at {scale}%")
                        }),
                )
            }
            (None, None) => None,
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .h(px(28.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(12.))
                    .text_size(px(FS_SM))
                    .child(
                        btn("screen-capture", "Capture", BtnKind::Plain, busy, &t)
                            .child(if busy {
                                spinner(13., t.text_muted).into_any_element()
                            } else {
                                icon("camera", 13.).into_any_element()
                            })
                            .child("Capture")
                            .when(!busy, |b| {
                                b.on_click(cx.listener(|this, _, _, cx| this.capture(cx)))
                            }),
                    )
                    .child(
                        btn("screen-live", "Live", BtnKind::Plain, false, &t)
                            .aria_toggled(if live { Toggled::True } else { Toggled::False })
                            .when(live, |b| b.bg(t.bg_selected_muted).text_color(fg))
                            .child("Live")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.screen.live = !this.screen.live;
                                if this.screen.live && !this.screen.busy {
                                    this.capture(cx);
                                }
                                cx.notify();
                            })),
                    )
                    .children(status),
            )
            .child(
                div()
                    .flex()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .px(px(12.))
                    .pb(px(8.))
                    .child({
                        let this = cx.entity().downgrade();
                        canvas(
                            move |bounds, _, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    // Less the picture's 1px border on each side.
                                    let room = bounds.size - Size::new(px(2.), px(2.));
                                    if this.screen.area != Some(room) {
                                        this.screen.area = Some(room);
                                        cx.notify();
                                    }
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .top_0()
                        .left(px(12.))
                        .right(px(12.))
                        .bottom(px(8.))
                    })
                    .children(self.screen.shot.as_ref().map(|(image, w, h)| {
                        img(image.clone())
                            .aria_label("The screen as the agent sees it")
                            .flex_none()
                            .w(px(*w as f32 * fit + 2.))
                            .h(px(*h as f32 * fit + 2.))
                            .object_fit(ObjectFit::Contain)
                            .rounded(px(RADIUS_SM))
                            .border_1()
                            .border_color(t.border)
                    })),
            )
            .into_any_element()
    }
}
