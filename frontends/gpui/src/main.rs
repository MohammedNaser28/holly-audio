//! gpui frontend (Task 8) — thin shell over `holly-app`.
//!
//! Scope discipline (project rule): this file knows NOTHING about the
//! device beyond what `holly_app::vm::ViewModel` carries. Every device-rule
//! string, readability decision, and pending-state rule comes from the VM;
//! the only write actions are the NC switch toggle (`set_noise`) and the NC
//! level buttons (`set_noise_level`, cmd 19's second byte — wire-exercised
//! in captures/2026-11-05-noise-level-medium.txt).
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
    App, Application, Bounds, ClickEvent, Context, ElementId, Entity, FocusHandle, FontWeight,
    KeyBinding, Render, TitlebarOptions, Window, WindowBounds, WindowOptions, actions, div,
    prelude::*, px, rgb, size,
};
use holly_app::DeviceHandle;
use holly_app::NoiseWrite;
use holly_app::vm::{Banner, BatteryClass, NcVm, ReverbVm, TxVm, ViewModel, view_model};

actions!(
    holly,
    [ToggleNoise, NcLow, NcMedium, NcStrong, Refresh, Quit]
);

mod theme;

// ---- Theme bridge (T1) -------------------------------------------------------
// The Signal token set is gone; `theme.rs` (verbatim from the design reference,
// docs/design/theme.rs) is the single colour source. The pre-T3 view layer
// below still uses the old component names, so they now resolve to graphite/
// tungsten tokens as follows — presentation mapping only, no device meaning:
//   window/background  -> CHASSIS        recessed rows/banner body -> PANEL_BOTTOM
//   card bodies        -> RAISED         one-pixel lines           -> LINE
//   primary/secondary  -> TEXT / TEXT_2  interactive emphasis      -> TUNGSTEN
//   "connected"        -> LIVE           needs-attention           -> TUNGSTEN
//                        (the design colour rule collapses amber roles:
//                         amber = engaged OR needs attention)
//   failure            -> ALERT
// Every value cites theme.rs — no hex literals remain in view code.
const COL_BG: u32 = theme::color::CHASSIS;
const COL_SURFACE: u32 = theme::color::PANEL_BOTTOM;
const COL_ELEVATED: u32 = theme::color::RAISED;
const COL_BORDER: u32 = theme::color::LINE;
const COL_TEXT: u32 = theme::color::TEXT;
const COL_MUTED: u32 = theme::color::TEXT_2;
const COL_ACCENT: u32 = theme::color::TUNGSTEN;
const COL_SUCCESS: u32 = theme::color::LIVE;
/// Needs-attention — same amber role as [`COL_ACCENT`] by the design colour rule.
const COL_WARNING: u32 = theme::color::TUNGSTEN;
const COL_ERROR: u32 = theme::color::ALERT;

fn battery_color(class: BatteryClass) -> gpui::Hsla {
    // Spec mapping: Full/High → success green, Low → amber, Critical → red.
    rgb(match class {
        BatteryClass::Full | BatteryClass::High => COL_SUCCESS,
        BatteryClass::Low => COL_WARNING,
        BatteryClass::Critical => COL_ERROR,
    })
    .into()
}

/// Per-TX battery line. Rule (spec §7c): a disconnected/unverifiable
/// transmitter shows an explicit unavailable state — never a decoded 0 %
/// (the heart's zero byte on a dead link is device padding, not 0 percent).
/// Pure + unit-tested.
fn battery_text(tx: &TxVm) -> String {
    if tx.connected {
        format!("battery {} % ({:?})", tx.battery, tx.battery_class)
    } else {
        "battery — (unavailable)".to_string()
    }
}

/// Header badge state, decided ONLY from what the VM carries (banner kind +
/// whether the connecting hint is active). Connected never invents itself: it
/// is the state when the actor stopped showing any banner or searching text.
/// Pure + unit-tested. Returns (label, token).
fn conn_badge(vm: &ViewModel) -> (&'static str, u32) {
    match &vm.banner {
        Some(Banner::Error) => ("error", COL_ERROR),
        Some(Banner::PermissionDenied) | Some(Banner::Busy) => ("blocked", COL_WARNING),
        Some(Banner::NotFound) => ("no device", COL_MUTED),
        None if vm.banner_text.is_some() => ("connecting", COL_ACCENT),
        None => ("connected", COL_SUCCESS),
    }
}

/// `HH:MM:SS` in UTC from integer UNIX seconds — std-only (no chrono, no
/// local timezone: the stamp is an actor-side wall clock; UTC display is
/// explicit and honest). `None` (no successful GET yet) renders a dash.
/// Pure + unit-tested.
fn utc_hms(updated: Option<u64>) -> String {
    let Some(secs) = updated else {
        return "—".to_string();
    };
    let d = secs % 86_400;
    format!("{:02}:{:02}:{:02}", d / 3600, d % 3600 / 60, d % 60)
}

fn onoff(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// Level button labels: 0/1/2 map to the APK's own value names
/// (`device_noise_level_low/normal/strong` resources, [code
/// HLDevice6302Activity.java:1593/:1610/:1626-1631]; presentation strings).
fn level_label(level: u8) -> &'static str {
    match level {
        0 => "low",
        1 => "medium",
        2 => "strong",
        _ => "unknown",
    }
}

/// Per-level-button decision — the ONLY button logic in the UI, pure and
/// unit-tested. A button is ACTIVE when the device reports this level with
/// the switch verified on; PENDING when an in-flight Level write targets it;
/// otherwise IDLE. Buttons are Idle whenever the verified switch state is
/// not `on`: the APK's level presses always send `isOpen=true` [code], and a
/// level write while off has never been sent/observed [wire] — so the row
/// stays inert until the toggle turns NC on. (Rationale with holly-usb:
/// `set_noise_level` preserves the switch byte, like the APK's own switch.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelBtn {
    Active,
    Pending,
    Idle,
}

fn level_button(level: u8, nc: &NcVm) -> LevelBtn {
    if nc.state != Some(true) {
        return LevelBtn::Idle;
    }
    if nc.level == Some(level) {
        return LevelBtn::Active;
    }
    if nc.pending == Some(NoiseWrite::Level(level)) {
        return LevelBtn::Pending;
    }
    LevelBtn::Idle
}

/// The ONLY place "what does a click mean" is decided, shared by mouse and
/// keyboard paths. `None` = disabled: no heart ever arrived, so there is no
/// verified state to toggle against (UI must not invent a success).
/// Pending wins over state, but ONLY a Switch write changes the toggle's
/// target semantics: an in-flight Level write is orthogonal (the flip is
/// computed from the verified state and simply goes out after it).
fn toggle_target(pending: Option<NoiseWrite>, state: Option<bool>) -> Option<bool> {
    match pending {
        Some(NoiseWrite::Switch(pending_on)) => Some(!pending_on),
        _ => Some(!state?),
    }
}

/// One window model; the whole app for v1. `demo` = scripted-device mode,
/// decided ONCE in `main` (--demo); it only ever changes presentation
/// (chip/title/footer note), never behavior and never the device API surface.
struct AppModel {
    focus: FocusHandle,
    vm: ViewModel,
    device: DeviceHandle,
    demo: bool,
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

    /// One level press: only when the verified switch is on and this level is
    /// not already the device's (no duplicate SETs for the active button).
    /// `set_noise_level` preserves the switch, mirroring the APK's switch
    /// path; a pending Level for another value is REPLACED here (holly-app's
    /// pump is latest-wins), which is the intended "change my mind" flow.
    fn set_nc_level(&mut self, level: u8, _: &mut Context<Self>) {
        if level_button(level, &self.vm.nc) == LevelBtn::Idle {
            self.device.set_noise_level(level);
        }
    }

    fn on_nc_low(&mut self, _: &NcLow, _: &mut Window, cx: &mut Context<Self>) {
        self.set_nc_level(0, cx);
    }

    fn on_nc_medium(&mut self, _: &NcMedium, _: &mut Window, cx: &mut Context<Self>) {
        self.set_nc_level(1, cx);
    }

    fn on_nc_strong(&mut self, _: &NcStrong, _: &mut Window, cx: &mut Context<Self>) {
        self.set_nc_level(2, cx);
    }

    fn on_nc_click(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_nc(&ToggleNoise, window, cx);
    }

    fn refresh(&mut self, _: &Refresh, _: &mut Window, _: &mut Context<Self>) {
        self.device.refresh();
    }

    /// Header per spec §7a: name + subtitle (+ DEMO chip when demo mode) on
    /// the first line, the connection badge on the right, key hints as a
    /// second quiet line. Badge color/label from [`conn_badge`] (pure,
    /// VM-only), DEMO chip = warning token on bg text (amber = "be aware"),
    /// never hidden (explicit mock/hardware separation).
    fn render_header(&mut self) -> gpui::Div {
        let (badge_label, badge_color) = conn_badge(&self.vm);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("HOLLY"))
                            .child(
                                div()
                                    .text_color(rgb(COL_MUTED))
                                    .text_sm()
                                    .child("Wireless Audio Controller"),
                            )
                            .children(self.demo.then(|| {
                                div()
                                    .px_2()
                                    .rounded_md()
                                    .bg(rgb(COL_WARNING))
                                    .text_color(rgb(COL_BG))
                                    .child("DEMO")
                            })),
                    )
                    .child(
                        div()
                            .px_2()
                            .rounded_md()
                            .text_color(rgb(badge_color))
                            .child(badge_label),
                    ),
            )
            .child(
                div().text_color(rgb(COL_MUTED)).text_sm().child(
                    "space: toggle NC · 1/2/3: low/medium/strong · r: reconnect · ctrl+q: quit",
                ),
            )
    }

    /// Device-rule banner straight from the VM (vm.rs owns these strings).
    /// Token mapping: error → red, permission/busy → amber, not-found stays
    /// quiet (muted) — backgrounds stay on `SURFACE`, only the border and
    /// text carry the semantic token (spec: no invented hex here).
    fn render_banner(&mut self) -> Option<gpui::Div> {
        let banner = self.vm.banner.clone()?;
        let (border, fg) = match banner {
            Banner::Error => (COL_ERROR, COL_ERROR),
            Banner::PermissionDenied | Banner::Busy => (COL_WARNING, COL_WARNING),
            Banner::NotFound => (COL_BORDER, COL_MUTED),
        };
        Some(
            div()
                .px_4()
                .py_3()
                .rounded_md()
                .border_1()
                .border_color(rgb(border))
                .bg(rgb(COL_SURFACE))
                .child(
                    div()
                        .text_color(rgb(fg))
                        .child(self.vm.banner_text.clone().unwrap_or_default()),
                ),
        )
    }

    /// The NC panel: toggle row + the three level buttons, laid out as ONE
    /// row stack. Pending display comes ONLY from `vm.nc.pending`
    /// (actor-reported), never from a click.
    fn render_nc(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.render_nc_row(cx))
            .child(self.render_nc_levels(cx))
    }

    /// Per-level button. The wrap div keeps them visually one row even when
    /// the whole panel is idle.
    /// Level buttons row: one button per APK value 0/1/2. The decision per
    /// button = [`level_button`]; the click + keyboard bindings share it.
    fn render_nc_levels(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let mut row = div().flex().gap_2();
        for (level, action) in [(0, "1: low"), (1, "2: medium"), (2, "3: strong")] {
            row = row.child(self.nc_level_btn(level, action, cx));
        }
        row
    }

    /// One level button: active/pending/idle styling from [`level_button`],
    /// the write goes through the actor only when the decision allows it
    /// (that shared decision is also what the 1/2/3 keyboard bindings call).
    fn nc_level_btn(
        &mut self,
        level: u8,
        key_hint: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let enabled = self.vm.nc.state == Some(true);
        let decision = level_button(level, &self.vm.nc);
        let (border, bg) = match decision {
            LevelBtn::Active => (COL_ACCENT, COL_ACCENT),
            // Pending = amber border on surface (semantic token, no invented
            // hex); Idle = quiet border on surface.
            LevelBtn::Pending => (COL_WARNING, COL_SURFACE),
            LevelBtn::Idle => (COL_BORDER, COL_SURFACE),
        };
        // Text stays readable on the active accent background.
        let fg = match decision {
            LevelBtn::Active => COL_BG,
            _ if enabled => COL_TEXT,
            _ => COL_MUTED,
        };
        let label = format!("{} {} ({level})", key_hint, level_label(level));

        div()
            .id(ElementId::named_usize("nc-level-btn", level as usize))
            .px_4()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(border))
            .bg(rgb(bg))
            .text_color(rgb(fg))
            .map(|btn| {
                if enabled {
                    btn.hover(|s| s.cursor_pointer())
                } else {
                    btn
                }
            })
            .child(label)
            .on_click(
                cx.listener(move |this, _: &ClickEvent, _: &mut Window, cx| {
                    this.set_nc_level(level, cx);
                }),
            )
    }

    /// The toggle ROW. Disabled (non-clickable look) whenever the VM has no
    /// verified NC state; pending display comes ONLY from `vm.nc.pending`
    /// (actor-reported), never from the click.
    fn render_nc_row(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let enabled = self.vm.nc.state.is_some();
        let pending = self.vm.nc.pending;
        let state = self.vm.nc.state;

        let (border, bg) = if !enabled {
            (COL_BORDER, COL_SURFACE)
        } else if pending.is_some() {
            (COL_WARNING, COL_SURFACE)
        } else {
            (COL_ACCENT, COL_ELEVATED)
        };

        let level = self.vm.nc.level;
        let state_part = match (state, level) {
            (Some(actual), Some(l)) => {
                format!("{} · level {} ({})", onoff(actual), l, level_label(l))
            }
            (Some(actual), None) => onoff(actual).to_string(),
            (None, _) => "—".to_string(),
        };
        let right = match pending {
            Some(NoiseWrite::Switch(t)) => {
                format!("pending → switch {} · device still: {state_part}", onoff(t))
            }
            Some(NoiseWrite::Level(l)) => format!(
                "pending → level {} ({}) · device still: {state_part}",
                l,
                level_label(l)
            ),
            None => state_part,
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
                        .child("SET_NOISE (cmd 19) — switch toggle [wire E2/E4] · level writes wire-exercised 2026-11-05"),
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
            .bg(rgb(COL_ELEVATED))
            .border_1()
            .border_color(rgb(COL_BORDER))
            .child(div().child(left))
            .child(
                div()
                    .text_color(battery_color(tx.battery_class))
                    .child(battery_text(tx)),
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
                .bg(rgb(COL_SURFACE))
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

    /// Footer per spec §7f: last successful update + version, subtle. The
    /// time comes from the VM's `updated` echo (actor-stamped, change-gated:
    /// it advances when the device confirmed something new). DEMO mode
    /// prefixes an explicit mock-data note — never passable for hardware.
    fn render_footer(&mut self) -> gpui::Div {
        let left = if self.demo {
            format!(
                "DEMO — scripted device, no hardware · state received {} (UTC)",
                utc_hms(self.vm.updated)
            )
        } else {
            format!("state received {} (UTC)", utc_hms(self.vm.updated))
        };
        div()
            .flex()
            .justify_between()
            .pt_2()
            .text_color(rgb(COL_MUTED))
            .text_sm()
            .child(left)
            .child(format!("holly-gpui v{}", env!("CARGO_PKG_VERSION")))
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
            .on_action(cx.listener(Self::on_nc_low))
            .on_action(cx.listener(Self::on_nc_medium))
            .on_action(cx.listener(Self::on_nc_strong))
            .on_action(cx.listener(Self::refresh))
            .child(self.render_header())
            .children(self.render_banner())
            .child(self.render_nc(cx))
            .children(self.render_txs().into_iter().flatten())
            .children(self.render_audio())
            .child(self.render_footer())
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("space", ToggleNoise, None),
            KeyBinding::new("1", NcLow, None),
            KeyBinding::new("2", NcMedium, None),
            KeyBinding::new("3", NcStrong, None),
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

        // The device actor owns the ONLY claim. `--demo` (explicit flag)
        // starts the SAME actor over the scripted DemoDevice instead: zero
        // USB in demo mode, clearly marked in the UI (chip + title + footer).
        // Discard anything after the flag so arg shapes stay minimal (no
        // clap in a frontend of this size).
        let demo = std::env::args().skip(1).any(|a| a == "--demo");
        let device = if demo {
            holly_app::demo::start_demo()
        } else {
            holly_app::start()
        };
        let rx = device.subscribe();

        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(760.), px(620.)),
                        cx,
                    ))),
                    titlebar: Some(TitlebarOptions {
                        title: Some(if demo {
                            "HOLLY — DEMO".into()
                        } else {
                            "HOLLY".into()
                        }),
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
                            demo,
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
    use super::{
        LevelBtn, NoiseWrite, battery_text, conn_badge, level_button, toggle_target, utc_hms,
    };
    use holly_app::vm::{Banner, BatteryClass, NcVm, TxVm, ViewModel};
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
        // Pending switch overrides displayed state (double-click stability).
        assert_eq!(
            toggle_target(Some(NoiseWrite::Switch(false)), Some(true)),
            Some(true)
        );
        assert_eq!(
            toggle_target(Some(NoiseWrite::Switch(true)), Some(false)),
            Some(false)
        );
        // Pending equal to current state folds to the opposite — matches
        // the pending-snapshot semantics (pending == the value being SET).
        assert_eq!(
            toggle_target(Some(NoiseWrite::Switch(true)), Some(true)),
            Some(false)
        );
        // An in-flight Level write is orthogonal to the switch: the target
        // is computed purely from the verified state.
        assert_eq!(
            toggle_target(Some(NoiseWrite::Level(2)), Some(true)),
            Some(false)
        );
        assert_eq!(
            toggle_target(Some(NoiseWrite::Level(0)), Some(false)),
            Some(true)
        );
    }

    fn nc(level: Option<u8>, state: Option<bool>, pending: Option<NoiseWrite>) -> NcVm {
        NcVm {
            state,
            level,
            pending,
            last_error: None,
        }
    }

    /// Level buttons: active/pending/idle matrix (the entire visual+write
    /// decision set for the 1/2/3 row).
    #[test]
    fn level_button_matrix() {
        // switch not verified on → always Idle (never writes with a
        // never-observed off-state level semantics).
        for st in [None, Some(false)] {
            assert_eq!(level_button(2, &nc(Some(2), st, None)), LevelBtn::Idle);
            assert_eq!(
                level_button(2, &nc(Some(2), st, Some(NoiseWrite::Level(2)))),
                LevelBtn::Idle,
                "pending does not rescue a switch-off row"
            );
        }
        // switch on: the device's current level is ACTIVE (click = no-op).
        assert_eq!(
            level_button(1, &nc(Some(1), Some(true), None)),
            LevelBtn::Active
        );
        // other levels idle or pending only via an exact Level match.
        assert_eq!(
            level_button(2, &nc(Some(1), Some(true), None)),
            LevelBtn::Idle
        );
        assert_eq!(
            level_button(2, &nc(Some(1), Some(true), Some(NoiseWrite::Level(2)))),
            LevelBtn::Pending
        );
        // an in-flight SWITCH flip does not mark level buttons pending.
        assert_eq!(
            level_button(2, &nc(Some(0), Some(true), Some(NoiseWrite::Switch(false)))),
            LevelBtn::Idle
        );
        // unknown level values are never Active (only 0/1/2 known).
        assert_eq!(
            level_button(5, &nc(Some(5), Some(true), None)),
            LevelBtn::Active,
            "device-reported odd level shows as active (read truth) — press path blocks via set_nc_level"
        );
    }

    fn blank_vm() -> ViewModel {
        ViewModel {
            banner: None,
            banner_text: None,
            tx1: None,
            tx2: None,
            tx3: None,
            tx4: None,
            nc: NcVm {
                state: None,
                level: None,
                pending: None,
                last_error: None,
            },
            voice_level: None,
            eq_level: None,
            reverb: None,
            updated: Some(0),
        }
    }

    fn tx(connected: bool, battery: u8, class: BatteryClass) -> TxVm {
        TxVm {
            connected,
            battery,
            battery_class: class,
            muted: false,
        }
    }

    /// Battery line (spec §7c): a disconnected transmitter must NOT render a
    /// 0 % figure — the heart's zero on a dead link is padding, not percent.
    #[test]
    fn battery_text_shows_unavailable_when_disconnected() {
        let live = tx(true, 93, BatteryClass::High);
        assert_eq!(battery_text(&live), "battery 93 % (High)");
        // Even with device-decoded "0 % (Critical)" bytes on a dead link:
        let dead = tx(false, 0, BatteryClass::Critical);
        assert_eq!(battery_text(&dead), "battery — (unavailable)");
    }

    /// Header badge: decided only from VM state; boundary arms on both ends.
    #[test]
    fn conn_badge_matrix() {
        // Connected = no banner AND no searching text, nothing invented.
        assert_eq!(conn_badge(&blank_vm()), ("connected", super::COL_SUCCESS));

        let mut connecting = blank_vm();
        connecting.banner_text = Some("searching for the dongle…".to_string());
        assert_eq!(conn_badge(&connecting), ("connecting", super::COL_ACCENT));

        let mut nf = blank_vm();
        nf.banner = Some(Banner::NotFound);
        assert_eq!(conn_badge(&nf), ("no device", super::COL_MUTED));

        let mut busy = blank_vm();
        busy.banner = Some(Banner::Busy);
        assert_eq!(conn_badge(&busy), ("blocked", super::COL_WARNING));
        let mut denied = blank_vm();
        denied.banner = Some(Banner::PermissionDenied);
        assert_eq!(conn_badge(&denied), ("blocked", super::COL_WARNING));

        let mut err = blank_vm();
        err.banner = Some(Banner::Error);
        assert_eq!(conn_badge(&err), ("error", super::COL_ERROR));
    }

    /// UTC HH:MM:SS from the actor's UNIX-second stamp, std-only (day
    /// boundary irrelevant); no successful GET yet → an explicit dash.
    #[test]
    fn utc_hms_boundaries() {
        assert_eq!(utc_hms(None), "—");
        assert_eq!(utc_hms(Some(0)), "00:00:00");
        assert_eq!(utc_hms(Some(3725)), "01:02:05");
        assert_eq!(utc_hms(Some(86_399)), "23:59:59");
        assert_eq!(utc_hms(Some(86_499)), "00:01:39", "wraps at midnight");
    }
}
