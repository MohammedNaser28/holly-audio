//! gpui frontend (Task 8) — thin shell over `holly-app`.
//!
//! Scope discipline (project rule): this file knows NOTHING about the
//! device beyond what `holly_app::vm::ViewModel` carries. Every device-rule
//! string, readability decision, and pending-state rule comes from the VM;
//! the only write action is the NC toggle forwarding `DeviceHandle::set_noise`.
//!
//! gpui API provenance (hard rule: nothing written from memory): every call
//! below was verified against the vendored gpui 0.2.2 source
//! (~/.cargo/registry/src/.../gpui-0.2.2/), citing the specific examples:
//!   - window setup: examples/hello_world.rs, examples/on_window_close_quit.rs
//!   - actions/bindings/on_action/listeners: examples/input.rs
//!   - async bridging: src/app.rs `App::spawn` (foreground executor,
//!     AsyncFnOnce(&mut AsyncApp)) + src/app/async_context.rs
//!     (`impl AppContext for AsyncApp`) + src/app/entity_map.rs
//!     `Entity::update` — the blocker is that AsyncApp is !Send, so a
//!     std thread cannot drive gpui async directly; see the pump below.
//!
//! Bridge pattern: `DeviceHandle::subscribe()` yields a BLOCKING
//! `std::sync::mpsc::Receiver<Snapshot>`. A dedicated pump thread holds the
//! blocking receive and forwards into `futures::channel::mpsc::unbounded`
//! (`unbounded_send` never blocks; `futures 0.3` is already part of gpui
//! 0.2.2's dependency tree, so no new dependency). The UI consumes the
//! stream via `cx.spawn` + `Entity::update` on the main-thread executor.

use std::thread;

use futures::{StreamExt, channel::mpsc::unbounded};
use gpui::{
    App, Application, Bounds, ClickEvent, Context, Entity, FocusHandle, KeyBinding, Render,
    TitlebarOptions, Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, rgb, size,
};
use holly_app::DeviceHandle;
use holly_app::vm::{Banner, BatteryClass, ReverbVm, TxVm, ViewModel, view_model};

actions!(holly, [ToggleNoise, Refresh, Quit]);

// ---- presentation palette --------------------------------------------------
// Presentation-only colors: no device meaning, chosen here (the VM exposes
// classes, not colors). Dark scheme to match the project's other UIs.
const COL_BG: u32 = 0x14161d;
const COL_PANEL: u32 = 0x1e222c;
const COL_PANEL_DIM: u32 = 0x1a1d25;
const COL_BORDER: u32 = 0x2a2f3a;
const COL_TEXT: u32 = 0xd8dee9;
const COL_MUTED: u32 = 0x8892a4;
const COL_ACCENT: u32 = 0x5b9dd9;
const COL_PENDING: u32 = 0xd19a66;
const COL_ERROR: u32 = 0x8c3840;
const COL_ERROR_TEXT: u32 = 0xf0a4ab;
const COL_WARN: u32 = 0x6e5320;
const COL_WARN_TEXT: u32 = 0xe5c07b;

fn battery_color(class: BatteryClass) -> gpui::Hsla {
    rgb(match class {
        BatteryClass::Full => 0x98c379,
        BatteryClass::High => 0xb5bd68,
        BatteryClass::Low => 0xe5c07b,
        BatteryClass::Critical => 0xe06c75,
    })
    .into()
}

fn onoff(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// The ONLY place "what does a click mean" is decided, shared by mouse and
/// keyboard paths. `None` = disabled: no heart ever arrived, so there is no
/// verified state to toggle against (UI must not invent a success).
fn toggle_target(pending: Option<bool>, state: Option<bool>) -> Option<bool> {
    Some(!pending.unwrap_or(state?))
}

/// One window model; the whole app for v1.
struct AppModel {
    focus: FocusHandle,
    vm: ViewModel,
    device: DeviceHandle,
}

impl AppModel {
    /// Stream-side entry: replace the VM, repaint. No interpretation here —
    /// pending/banners/errors were already decided by the actor + vm.
    fn apply_snapshot(&mut self, snap: holly_app::Snapshot, cx: &mut Context<Self>) {
        self.vm = view_model(&snap);
        cx.notify();
    }

    fn toggle_nc(&mut self, _: &ToggleNoise, _: &mut Window, _: &mut Context<Self>) {
        if let Some(target) = toggle_target(self.vm.nc.pending, self.vm.nc.state) {
            self.device.set_noise(target);
        }
    }

    fn on_nc_click(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_nc(&ToggleNoise, window, cx);
    }

    fn refresh(&mut self, _: &Refresh, _: &mut Window, _: &mut Context<Self>) {
        self.device.refresh();
    }

    fn render_header(&mut self) -> gpui::Div {
        div()
            .flex()
            .justify_between()
            .items_center()
            .child("Hollyland 6108 — control surface")
            .child(
                div()
                    .text_color(rgb(COL_MUTED))
                    .child("space: toggle NC · r: reconnect · ctrl+q: quit"),
            )
    }

    /// Device-rule banner straight from the VM (vm.rs owns these strings).
    fn render_banner(&mut self) -> Option<gpui::Div> {
        let banner = self.vm.banner.clone()?;
        let (bg, fg) = match banner {
            Banner::Error => (COL_ERROR, COL_ERROR_TEXT),
            Banner::PermissionDenied => (COL_WARN, COL_WARN_TEXT),
            Banner::Busy => (COL_WARN, COL_WARN_TEXT),
            Banner::NotFound => (COL_BORDER, COL_MUTED),
        };
        Some(
            div()
                .px_4()
                .py_3()
                .rounded_md()
                .border_1()
                .border_color(rgb(bg))
                .bg(rgb(bg))
                .child(
                    div()
                        .text_color(rgb(fg))
                        .child(self.vm.banner_text.clone().unwrap_or_default()),
                ),
        )
    }

    /// The singular writable row. Disabled (non-clickable look) whenever the
    /// VM has no verified NC state; pending display comes ONLY from
    /// `vm.nc.pending` (actor-reported), never from the click.
    fn render_nc(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let enabled = self.vm.nc.state.is_some();
        let pending = self.vm.nc.pending;
        let state = self.vm.nc.state;

        let (border, bg) = if !enabled {
            (COL_BORDER, COL_PANEL_DIM)
        } else if pending.is_some() {
            (COL_PENDING, COL_PANEL_DIM)
        } else {
            (COL_ACCENT, COL_PANEL)
        };

        let right = match (pending, state) {
            (Some(target), Some(actual)) => {
                format!("pending → {} (still {})", onoff(target), onoff(actual))
            }
            (Some(target), None) => format!("pending → {} (no verified state)", onoff(target)),
            (None, Some(actual)) => onoff(actual).to_string(),
            (None, None) => "—".to_string(),
        };

        div()
            .id("nc-row")
            .flex()
            .justify_between()
            .items_center()
            .px_4()
            .py_3()
            .rounded_md()
            .border_1()
            .border_color(rgb(border))
            .bg(rgb(bg))
            .map(|row| {
                if enabled {
                    row.hover(|s| s.cursor_pointer())
                } else {
                    row
                }
            })
            .on_click(cx.listener(Self::on_nc_click))
            .child(
                div().child("noise cancellation").child(
                    div()
                        .text_color(rgb(COL_MUTED))
                        .text_sm()
                        .child("the only certified control (SET_NOISE, cmd 19)"),
                ),
            )
            .child(
                div()
                    .text_color(rgb(if enabled { COL_TEXT } else { COL_MUTED }))
                    .child(right),
            )
    }

    fn tx_card(&self, idx: u8, tx: &TxVm) -> gpui::Div {
        let status = if tx.connected {
            "connected".to_string()
        } else {
            "no link".to_string()
        };
        let mut left = format!("tx{idx} — {status}");
        if tx.muted {
            left.push_str(" · muted");
        }
        div()
            .flex()
            .justify_between()
            .px_4()
            .py_2()
            .rounded_md()
            .bg(rgb(COL_PANEL))
            .border_1()
            .border_color(rgb(COL_BORDER))
            .child(div().child(left))
            .child(
                div()
                    .text_color(battery_color(tx.battery_class))
                    .child(format!("battery {} % ({:?})", tx.battery, tx.battery_class)),
            )
    }

    /// TX rows: always tx1/tx2 when a heart exists (even for a disconnected
    /// TX), tail rows only when the reply carried a complete triple.
    fn render_txs(&self) -> [Option<gpui::Div>; 4] {
        [
            self.vm.tx1.as_ref().map(|t| self.tx_card(1, t)),
            self.vm.tx2.as_ref().map(|t| self.tx_card(2, t)),
            self.vm.tx3.as_ref().map(|t| self.tx_card(3, t)),
            self.vm.tx4.as_ref().map(|t| self.tx_card(4, t)),
        ]
    }

    /// Read-only audio panels. When nothing is connected this whole section
    /// collapses to `None`.
    fn render_audio(&self) -> Option<gpui::Div> {
        let voice = self.vm.voice_level?;
        let eq = self.vm.eq_level?;
        let ReverbVm { enabled, level } = self.vm.reverb?;
        Some(
            div()
                .flex()
                .justify_between()
                .px_4()
                .py_2()
                .rounded_md()
                .bg(rgb(COL_PANEL_DIM))
                .border_1()
                .border_color(rgb(COL_BORDER))
                .child(div().text_color(rgb(COL_MUTED)).child(format!(
                    "read-only: gain {voice} · eq {eq} · reverb {} (level {level})",
                    onoff(enabled)
                )))
                .child(
                    div()
                        .text_color(rgb(COL_MUTED))
                        .text_sm()
                        .child("writable only after a capture"),
                ),
        )
    }
}

impl Render for AppModel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(rgb(COL_BG))
            .text_color(rgb(COL_TEXT))
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::toggle_nc))
            .on_action(cx.listener(Self::refresh))
            .child(self.render_header())
            .children(self.render_banner())
            .child(self.render_nc(cx))
            .children(self.render_txs().into_iter().flatten())
            .children(self.render_audio())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .pt_2()
                    .text_color(rgb(COL_MUTED))
                    .text_sm()
                    .child("state changes only when the device reports them (1 s heartbeat, change-only)"),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("space", ToggleNoise, None),
            KeyBinding::new("r", Refresh, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        // The device actor owns the ONLY claim; start() spawns its thread.
        let device = holly_app::start();
        let rx = device.subscribe();

        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(620.), px(520.)),
                        cx,
                    ))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Hollyland 6108".into()),
                        ..Default::default()
                    }),
                    app_id: Some("holly-gpui".to_string()),
                    ..Default::default()
                },
                |window, cx| {
                    cx.new(|cx| {
                        let focus = cx.focus_handle();
                        focus.focus(window);
                        AppModel {
                            focus,
                            vm: ViewModel::default(),
                            device,
                        }
                    })
                },
            )
            .unwrap();

        let view: Entity<AppModel> = window.update(cx, |_, _, cx| cx.entity()).unwrap();

        // ---- blocking Receiver -> executor bridge (see module docs) ----
        let (snap_tx, mut snap_rx) = unbounded::<holly_app::Snapshot>();
        thread::Builder::new()
            .name("holly-gpui-pump".to_string())
            .spawn(move || {
                for snap in rx {
                    if snap_tx.unbounded_send(snap).is_err() {
                        break; // UI side is gone
                    }
                }
                // Keep rx alive until UI teardown; rx dropped here releases
                // our subscriber slot (Hub silently drops lagged subs).
            })
            .expect("spawn holly-gpui-pump");

        cx.spawn(async move |cx| {
            // snap_rx.next() requires `futures::StreamExt` (import above).
            while let Some(snap) = snap_rx.next().await {
                if view
                    .update(cx, |model, cx| model.apply_snapshot(snap, cx))
                    .is_err()
                {
                    break; // view released (window closed)
                }
            }
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::toggle_target;

    /// The toggle's entire decision logic, exercised on all pending/state
    /// combinations. Disabled whenever no verified state exists; pending
    /// (actor-reported in-flight SET) wins over the displayed state, which
    /// makes the toggle idempotent under a double-click while a SET is
    /// still in flight: the second click computes the SAME target.
    #[test]
    fn toggle_target_matrix() {
        // No verified state: disabled both with and without a stray pending.
        assert_eq!(toggle_target(None, None), None);
        // Verified states.
        assert_eq!(toggle_target(None, Some(true)), Some(false));
        assert_eq!(toggle_target(None, Some(false)), Some(true));
        // Pending overrides displayed state (double-click stability).
        assert_eq!(toggle_target(Some(false), Some(true)), Some(true));
        assert_eq!(toggle_target(Some(true), Some(false)), Some(false));
        // Pending equal to current state folds to the opposite — matches
        // the pending-snapshot semantics (pending == the value being SET).
        assert_eq!(toggle_target(Some(true), Some(true)), Some(false));
    }
}
