//! The panel views (pure move out of main.rs, commit C): the window chrome
//! (titlebar, banner, footer), the body grid, the mic cards, the NC panel
//! with its rocker, and every control panel (NC level, EQ, Volume, Channel
//! mode, Schedule, Reverb) with the power-row switches.
//!
//! The methods live in a second `impl AppModel` block: the struct and the
//! action handlers stay in main.rs; panels borrow the same VM fields. The
//! device rules and pending-state wording still come ONLY from the VM —
//! nothing here names a wire value.

use gpui::{
    BoxShadow, ClickEvent, Context, FontWeight, MouseButton, div, linear_color_stop,
    linear_gradient, point, prelude::*, px,
};

use holly_app::vm::{
    Banner, BatteryClass, DEVICE_NAME, TxVm, VID_PID, battery_segment_count, conn_status_word,
    mic_status, mute_label, nc_level_label, nc_status, reverb_selection,
};

use crate::AppModel;
use crate::controls::{
    ChannelPick, EqPick, LevelPick, ReverbPick, SchedulePick, VolumePick, banner_palette,
    battery_digits, channel_pick, eq_pick, level_pick, light_toggle_accepted,
    mic_mute_write_accepted, mic_toggle_accepted, nc_led, reverb_pick, reverb_toggle_accepted,
    schedule_pick, vm_led, volume_pick,
};
use crate::theme;
use crate::widgets::{
    ICON_CHARGER, ICON_CLOSE, ICON_MIC, LedLit, SelectorSpec, led_dot, matte_under_edge, panel,
    segmented_selector, solid, svg_glyph, titled_with_led, toggle_shell, well,
};

impl AppModel {
    // ---- titlebar (44px) ------------------------------------------------------

    /// Reference titlebar: 44px, gap 10, padding left 16, bottom rule
    /// LINE_SOFT; logo (TEXT ring + tungsten dot), "Holly" 14/600, a 1×16
    /// LINE divider, then `DEVICE_NAME` 14/TEXT_2 (VM const). The left
    /// portion is ONE drag strip (`start_window_move` on mouse-down); the
    /// close button stays OUTSIDE it. CSD context: with client decorations
    /// the compositor renders no window buttons, so the close affordance
    /// is a platform necessity the painted mock does not depict.
    pub(crate) fn render_titlebar(&self) -> gpui::Div {
        div()
            .h(px(theme::metrics::TITLEBAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .pl(px(16.))
            .pr(px(12.))
            .border_b_1()
            .border_color(solid(theme::color::LINE_SOFT))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    // drag strip (logo .. device name / DEMO chip)
                    .on_mouse_down(MouseButton::Left, |_, window, _| {
                        window.start_window_move();
                    })
                    .child(
                        // logo: the reference's 18×18 SVG has a stroke ring AND a
                        // filled dot of DIFFERENT colors, while `svg()` paints one
                        // color from an alpha mask — so it is composed from div
                        // primitives instead (README: reused assets). Geometry
                        // follows the svg's proportional rendering
                        // (viewBox 24 → 18px: r10 ring ≈16px diameter, stroke
                        // 1.6×.75 ≈1.2px, r4 dot ≈6px — index.html:31).
                        div()
                            .w(px(16.))
                            .h(px(16.))
                            .rounded_full()
                            .border(px(1.2)) // absolute-length border setter (gpui-macros styles.rs:348-360; "border" = all four edges, styles.rs:1325-1334)
                            .border_color(solid(theme::color::TEXT))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .w(px(6.))
                                    .h(px(6.))
                                    .rounded_full()
                                    .bg(solid(theme::color::TUNGSTEN)),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Holly"),
                    )
                    .child(div().w(px(1.)).h(px(16.)).bg(solid(theme::color::LINE)))
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(solid(theme::color::TEXT_2))
                            .child(DEVICE_NAME),
                    )
                    .children(self.demo.map(|_| {
                        div()
                            .ml(px(6.))
                            .px(px(6.))
                            .rounded(px(3.))
                            .bg(solid(theme::color::TUNGSTEN))
                            .text_color(solid(theme::color::INK))
                            .text_size(px(theme::text::CAPTION_SIZE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("DEMO")
                    })),
            )
            .child(div().flex_1())
            .child(
                // close button (outside the drag strip; CSD)
                div()
                    .id("titlebar-close")
                    .w(px(28.))
                    .h(px(28.))
                    .rounded(px(theme::metrics::BUTTON_RADIUS))
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(solid(theme::color::RAISED)))
                    .on_click(|_, window, _| window.remove_window())
                    .child(svg_glyph(ICON_CLOSE, 14., solid(theme::color::TEXT_2))),
            )
    }

    // ---- banner ---------------------------------------------------------------

    /// Device-rule banner straight from the VM (`conn_status_word` fills
    /// the title slot, `banner_text` the body: mind that the reference's
    /// banner words differ — the VM wording wins per project rule).
    /// Reference recipe: alert row, 14px gap, 12/20 padding, BANNER_BG
    /// fill + bottom rule; title 14/600; body 13; "Try again" button
    /// (RAISED fill, LINE border, 36px) on the right. Shows for every
    /// banner kind (it is the only recovery affordance).
    pub(crate) fn render_banner(&self, cx: &Context<Self>) -> Option<gpui::Div> {
        let banner = self.vm.banner.as_ref()?;
        let palette = banner_palette(banner);
        let body = self.vm.banner_text.clone().unwrap_or_default();
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(14.))
                .px(px(20.))
                .py(px(12.))
                .bg(solid(theme::color::BANNER_BG))
                .border_b_1()
                .border_color(if banner == &Banner::Error {
                    solid(theme::color::ALERT)
                } else {
                    solid(theme::color::BANNER_LINE)
                })
                .child(svg_glyph(ICON_CHARGER, 22., solid(palette.accent)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(solid(palette.title))
                                .child(conn_status_word(&self.vm)),
                        )
                        .child(
                            div()
                                .text_size(px(theme::text::BODY_SIZE))
                                .text_color(solid(palette.body))
                                .child(body),
                        ),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .id("banner-retry")
                        .h(px(36.))
                        .px(px(16.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .rounded(px(theme::metrics::BUTTON_RADIUS))
                        .border_1()
                        .border_color(solid(theme::color::LINE))
                        .bg(solid(theme::color::RAISED))
                        .text_color(solid(theme::color::TEXT))
                        .text_size(px(theme::text::BODY_SIZE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .hover(|s| s.cursor_pointer())
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.refresh_now(cx);
                        }))
                        .child("Try again"),
                ),
        )
    }

    // ---- body -----------------------------------------------------------------

    /// Reference body: flex-1, gap 16, padding 20. Left = the 336px mic
    /// column (two equal cards); right = NC panel over its two fixed rows
    /// (132 / 120), each row split into the same two columns.
    pub(crate) fn render_body(&self, cx: &Context<Self>) -> gpui::Div {
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .gap(px(theme::metrics::SPACE_4))
            .p(px(theme::metrics::SPACE_5))
            .child(
                // left: Mic 1 / Mic 2 (336px)
                div()
                    .w(px(theme::metrics::MIC_COLUMN_WIDTH))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(theme::metrics::SPACE_4))
                    .child(self.mic_card("mic-1", 0, self.vm.tx1.as_ref(), cx).flex_1())
                    .child(self.mic_card("mic-2", 1, self.vm.tx2.as_ref(), cx).flex_1()),
            )
            .child(
                // right: rows 1fr / 132 / 120
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(theme::metrics::SPACE_4))
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .flex()
                            .gap(px(theme::metrics::SPACE_4))
                            .child(self.nc_panel(cx).flex_1())
                            .child(
                                div()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(theme::metrics::SPACE_4))
                                    .child(
                                        // Interactive since the gain round trip
                                        // (2026-11-10): the six-stop selector route
                                        // moved to volume_panel.
                                        self.volume_panel(cx).flex_1(),
                                    )
                                    .child(
                                        // Interactive since the channel-mode
                                        // round trip (2026-11-11): the two-mode
                                        // selector route moved to channel_panel.
                                        self.channel_panel(cx).flex_1(),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .h(px(theme::metrics::GRID_ROW_EQ_HEIGHT))
                            .flex_none()
                            .flex()
                            .gap(px(theme::metrics::SPACE_4))
                            .child(self.eq_panel(cx).flex_1())
                            .child(self.reverb_panel(cx).flex_1()),
                    )
                    .child(
                        div()
                            .h(px(theme::metrics::GRID_ROW_POWER_HEIGHT))
                            .flex_none()
                            .flex()
                            .gap(px(theme::metrics::SPACE_4))
                            .child(self.schedule_panel(cx).flex_1())
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(theme::metrics::SPACE_4))
                                    .child(
                                        self.led_toggle_panel(
                                            "Mic recognition",
                                            // §5 offset 15 read (`vm.mic_recognition`), not
                                            // a hardcoded false — INTERACTIVE since the
                                            // 2026-11-12 round trip; the switch arrives
                                            // prebuilt from [`Self::mic_toggle`].
                                            self.mic_toggle(cx).into_any_element(),
                                        )
                                        .flex_1(),
                                    )
                                    .child(
                                        self.led_toggle_panel(
                                            "Indicator light",
                                            self.light_toggle(cx).into_any_element(),
                                        )
                                        .flex_1(),
                                    ),
                            ),
                    ),
            )
    }

    /// One mic card (336px column, halves): reference recipe — the PANEL
    /// shell itself (20px padding, 14px radius, border + faceplate gradient
    /// — index.html:52; the interactive rewrite had rendered the card as a
    /// bare column, commit J restores the shell), icon in a
    /// WELL circle, name 15/600, LED + status (`mic_status`), mute chip
    /// (state read `[wire read]`; the chip is click-interactive only while
    /// [`mic_mute_write_accepted`] passes — family gate + connectedness —
    /// and dims to the existing 0.5 while ITS write is in flight), the
    /// 64px battery readout (LIGHT, "--" when unavailable), 5 battery
    /// segments, and an UNLIT 20-segment level meter (no wire source for
    /// the meter: always unlit, per the writability/evidence rules).
    /// Battery and level are SEPARATE card children (the card's own 18px
    /// gap separates them — index.html:52; the battery column's internal
    /// 10px gap lives in [`battery_block`], index.html:64).
    fn mic_card(
        &self,
        id: &'static str,
        slot: usize,
        tx: Option<&TxVm>,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let (name, status, muted, digits, live, lit) = match tx {
            Some(tx) => (
                name_for(id),
                mic_status(tx.connected).to_string(),
                mute_label(tx.muted),
                battery_digits(tx),
                tx.connected,
                if tx.connected {
                    battery_segment_count(tx.battery)
                } else {
                    0
                },
            ),
            None => (
                name_for(id),
                mic_status(false).to_string(),
                "Mute",
                "--".to_string(),
                false,
                0,
            ),
        };
        let glyph_color = solid(if live {
            theme::color::TEXT
        } else {
            theme::color::TEXT_3
        });
        // The card itself stays non-interactive; its child chip carries
        // the affordance (svg needs none). The chip is keyed by its card
        // ("mic-1-mute"/"mic-2-mute") — `.id` is required for the gated
        // click listener, the reverb-toggle shape: the listener attaches
        // ONLY while the whole gate passes, so a read-only family or an
        // in-flight write leaves the chip inert without new visuals.
        let chip_id: &'static str = if slot == 0 {
            "mic-1-mute"
        } else {
            "mic-2-mute"
        };
        let applying = tx.is_some_and(|tx| tx.applying);
        let mut chip = div()
            .id(chip_id)
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(36.))
            .px(px(12.))
            .rounded(px(theme::metrics::BUTTON_RADIUS))
            .border_1()
            .border_color(solid(theme::color::LINE))
            .bg(solid(theme::color::RAISED))
            .text_color(solid(theme::color::TEXT))
            .text_size(px(theme::text::BODY_SIZE))
            .font_weight(FontWeight::SEMIBOLD)
            // the family's existing in-flight look (0.5), per-slot pending
            .when(applying, |chip| chip.opacity(0.5))
            .child(svg_glyph(ICON_MIC, 14., solid(theme::color::TEXT)))
            .child(muted);
        if let Some(tx) = tx
            && mic_mute_write_accepted(tx)
        {
            chip = chip
                .on_click(cx.listener(move |this, _: &ClickEvent, _, _| this.flip_mic_mute(slot)));
        }
        // The card = the panel recipe itself (padding 20, radius 14,
        // border + gradient — index.html:52), `min-height:0` riding with
        // the call site's flex_1; gap 18 between header / battery / level.
        panel()
            .min_h(px(0.))
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .w(px(32.))
                            .h(px(32.))
                            .rounded_full()
                            .border_1()
                            .border_color(solid(theme::color::LINE_SOFT))
                            .bg(solid(theme::color::WELL))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(svg_glyph(ICON_MIC, 16., glyph_color)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(theme::text::TITLE_SIZE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_color(solid(theme::color::TEXT_2))
                                    .child(if live {
                                        led_dot(LedLit::Live)
                                    } else {
                                        led_dot(LedLit::Ring)
                                    })
                                    .child(status),
                            ),
                    )
                    .child(div().flex_1())
                    .child(chip),
            )
            .child(battery_block(
                &digits,
                live,
                lit,
                tx.map(|tx| tx.battery_class),
            ))
            .child(level_meter_block())
    }

    /// NC panel: title + LED/status row, then the rocker well + the level
    /// selector. While ANY SET is pending the status word says "Applying"
    /// (VM), the lit pad renders at 40% element opacity with no glow, the
    /// selector dims to 0.5, and BOTH controls ignore picks (mouse AND
    /// keyboard — the decisions live in `nc_write_accepted`/`level_pick`).
    fn nc_panel(&self, cx: &Context<Self>) -> gpui::Div {
        let nc = &self.vm.nc;
        let pending = nc.pending.is_some();
        div().flex_1().min_h(px(0.)).child(
            panel()
                .flex_1()
                .min_h(px(0.))
                .gap(px(18.))
                .child(titled_with_led(
                    "Noise cancellation",
                    nc_led(nc),
                    nc_status(nc).to_string(),
                ))
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .flex()
                        .gap(px(theme::metrics::SPACE_4))
                        .child(
                            // rocker well: 14px padding, 26px radius over
                            // the WELL fill (index.html:92). NOT a click
                            // target — the two pads own the clicks, and
                            // Space/Enter flip through the "nc_rocker"
                            // key context (bindings in main).
                            well(theme::metrics::WELL_RADIUS)
                                .items_center()
                                .justify_center()
                                .p(px(14.))
                                .track_focus(&self.focus)
                                .key_context("nc_rocker")
                                // focus-visible outline from the mock
                                // (2px tungsten, index.html:14) becomes a
                                // 2px border on the well itself.
                                .focus(|s| s.border_2().border_color(solid(theme::color::TUNGSTEN)))
                                .child(
                                    div()
                                        .w(px(theme::metrics::ROCKER_WIDTH))
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .child(self.rocker_pad_on(pending, cx))
                                        .child(self.rocker_pad_off(cx)),
                                ),
                        )
                        .child(self.nc_selector(pending, cx)),
                ),
        )
    }

    /// The On pad: the reference's lit recipe (tungsten gradient + INK text +
    /// 32px tungsten glow) exactly when the verified state is ON; while a SET
    /// is in flight the pad renders the SAME gradient at 40% ELEMENT opacity
    /// with no glow (index.html:238 — opacity on the pad, not the fill);
    /// OFF/unknown render the matte recipe with its hard under-edge. The
    /// click requests THIS pad's state; `nc_write_accepted` turns a
    /// pending-time click (or an unverified link) into a noop.
    fn rocker_pad_on(&self, pending: bool, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let tungsten_gradient = linear_gradient(
            180.,
            linear_color_stop(solid(theme::color::TUNGSTEN_TOP), 0.),
            linear_color_stop(solid(theme::color::TUNGSTEN_BOTTOM), 1.),
        );
        let matte_gradient = linear_gradient(
            180.,
            linear_color_stop(solid(theme::color::RAISED_TOP), 0.),
            linear_color_stop(solid(theme::color::RAISED_BOTTOM), 1.),
        );
        let pad = div()
            .id("nc-pad-on")
            .h(px(theme::metrics::ROCKER_PAD_ON_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .rounded_tl(px(theme::metrics::PAD_RADIUS_OUTER))
            .rounded_tr(px(theme::metrics::PAD_RADIUS_OUTER))
            .rounded_bl(px(theme::metrics::PAD_RADIUS_INNER))
            .rounded_br(px(theme::metrics::PAD_RADIUS_INNER))
            .text_size(px(16.))
            .font_weight(FontWeight::SEMIBOLD)
            .child("On");
        let pad = if pending {
            pad.text_color(solid(theme::color::INK))
                .bg(tungsten_gradient)
                .opacity(0.4)
        } else if self.vm.nc.state == Some(true) {
            // lit pad glow: `0 0 32px rgba(255,181,71,.35)` (theme.rs
            // GLOW_TUNGSTEN); the inset top highlight is dropped (README).
            pad.text_color(solid(theme::color::INK))
                .bg(tungsten_gradient)
                .shadow(vec![BoxShadow {
                    color: gpui::Hsla::from(solid(theme::color::GLOW_TUNGSTEN.0))
                        .opacity(theme::color::GLOW_TUNGSTEN.1),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(32.),
                    spread_radius: px(0.),
                }])
        } else {
            pad.text_color(solid(theme::color::TEXT_2))
                .bg(matte_gradient)
                .shadow(matte_under_edge(2.))
        };
        pad.on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            this.request_nc(true, window, cx);
        }))
    }

    /// The Off pad: always the matte recipe (the reference never lights it;
    /// a verified OFF is told apart by the status word). Requests OFF.
    fn rocker_pad_off(&self, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        div()
            .id("nc-pad-off")
            .h(px(theme::metrics::ROCKER_PAD_OFF_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .rounded_tl(px(theme::metrics::PAD_RADIUS_INNER))
            .rounded_tr(px(theme::metrics::PAD_RADIUS_INNER))
            .rounded_bl(px(theme::metrics::PAD_RADIUS_OUTER))
            .rounded_br(px(theme::metrics::PAD_RADIUS_OUTER))
            .bg(linear_gradient(
                180.,
                linear_color_stop(solid(theme::color::RAISED_TOP), 0.),
                linear_color_stop(solid(theme::color::RAISED_BOTTOM), 1.),
            ))
            .text_color(solid(theme::color::TEXT_2))
            .text_size(px(16.))
            .font_weight(FontWeight::SEMIBOLD)
            .shadow(matte_under_edge(2.))
            .child("Off")
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.request_nc(false, window, cx);
            }))
    }

    /// NC level selector: the reference is a VERTICAL radiogroup (flex
    /// column, index.html:98) — 4px padding, 2px gaps, each row flex-1 at
    /// min-height 0, radius 12/8. The selected option = the device's
    /// VERIFIED level (`nc_state == on && nc.level`); no pending target is
    /// ever shown (the device hasn't confirmed it). While ANY SET is pending
    /// the group dims to 0.5 (`nc.levelOpacity`, index.html:246) and picks
    /// are ignored (`level_pick`, shared with the 1/2/3 keys); with the
    /// switch verified OFF the rows render unselected and
    /// non-interactive (level-while-off was never sent/observed).
    fn nc_selector(&self, pending: bool, cx: &Context<Self>) -> gpui::Div {
        let nc = &self.vm.nc;
        let labels: [&str; 3] = [nc_level_label(0), nc_level_label(1), nc_level_label(2)];
        segmented_selector(
            SelectorSpec {
                id_prefix: "nc-level",
                labels: &labels,
                // The selected option = the device's VERIFIED level
                // (`nc_state == on && nc.level`); no pending target is ever
                // shown (the device hasn't confirmed it).
                is_selected: &|index| nc.state == Some(true) && nc.level == Some(index as u8),
                // [`level_pick`] is the whole decision (blocked while ANY
                // SET is pending, blocked with the switch verified off —
                // level-while-off was never sent/observed —, noop on the
                // device's own level), shared by mouse AND the 1/2/3 keys.
                is_pickable: &|index| level_pick(index as u8, nc) != LevelPick::Blocked,
                on_pick: |this, index, cx| this.set_nc_level(index as u8, cx),
                horizontal: false,
                dim: pending,
                // The NC level selector has NO keyboard scope of its own:
                // the rocker well owns "nc_rocker" (Space/Enter) and the
                // levels sit on the global 1/2/3 bindings.
                focus: None,
            },
            cx,
        )
    }

    /// EQ panel — the reference's titled panel + selector row, now
    /// INTERACTIVE and in the mock's HORIZONTAL orientation (options 36px
    /// tall, indicator bar 4px up — index.html:125-131; the interactive
    /// rewrite had dropped the panel shell + "EQ adjustment" heading AND
    /// copied the NC rocker's vertical bare-well stack, commit H restores
    /// both): picks send `set_eq` (wire pads via `eq_pad_wire`) through the
    /// same actor path as the NC controls, gated by `eq_pick`. While an EQ
    /// write is in flight (`EqVm.applying`) the selector dims to 0.5 and
    /// both mouse and keyboard picks are ignored — the snapshot's POST-SET
    /// poll moves the selection (no pending target is ever shown; same rule
    /// as the NC level selector). Left/Right step the pads while the well
    /// is focused ("eq_selector" key context — bindings in main); the
    /// DEVISION dim (busy → 0.6) intentionally NOT applied (README).
    fn eq_panel(&self, cx: &Context<Self>) -> gpui::Div {
        let eq = &self.vm.eq;
        // Display order = APK visual order [balance, overcast, bright] =
        // wire bytes [3,1,2] [code DeviceConnectVariable.EQ + reference
        // radio "Equalization/Low/Bright" (hldevice_6108.xml:156-166,
        // :1290-1292; index.html:262)] — translation is the VM's tested
        // `eq_pad_wire`, never a literal here.
        const EQ_LABELS: [&str; 3] = ["Equalization", "Low", "Bright"];
        let group = segmented_selector(
            SelectorSpec {
                id_prefix: "eq-pad",
                labels: &EQ_LABELS,
                // The device's verified selection — no pending target is
                // ever shown (the snapshot's POST-SET poll moves it).
                is_selected: &|index| eq.selected == Some(index),
                // [`eq_pick`] is the whole decision (read-only family or an
                // in-flight EQ write: blocked; already-selected: noop; else
                // write), shared by mouse AND the Left/Right keys.
                is_pickable: &|index| eq_pick(index, eq) != EqPick::Blocked,
                on_pick: |this, index, cx| this.set_eq_pad(index, cx),
                horizontal: true,
                dim: eq.applying,
                focus: Some((&self.eq_focus, "eq_selector")),
            },
            cx,
        );
        // The mock keeps the EQ panel's own titled shell around the
        // radiogroup (index.html:125-127 — same recipe as the Volume/
        // Channel/Schedule panels; unlike those the panel itself carries no
        // `flex:1`, so the grow comes from the grid cell).
        panel()
            .justify_center()
            .gap(px(theme::metrics::SPACE_3))
            .child(
                div()
                    .text_size(px(theme::text::TITLE_SIZE))
                    .font_weight(FontWeight::SEMIBOLD)
                    // [code index.html:126]
                    .child("EQ adjustment"),
            )
            .child(group)
    }

    /// Volume panel — the reference's title + six-stop selector row, now
    /// INTERACTIVE and in the mock's HORIZONTAL orientation (options 36px
    /// tall, indicator bar 4px up — index.html:106-112; the interactive
    /// rewrite had copied the NC rocker's vertical stack, commit F restores
    /// the designed row): picks send `set_gain` (wire stops via
    /// `volume_pad_wire`) through the same actor path as the NC/EQ
    /// controls, gated by `volume_pick`. While a gain write is in flight
    /// (`GainVm.applying`) the selector dims to 0.5 and both mouse and
    /// keyboard picks are ignored — the snapshot's POST-SET poll moves the
    /// selection (no pending target is ever shown; same rule as the EQ
    /// selector). Stop order IS byte order ("1".."6" [code index.html:260];
    /// byte N = stop N+1, §5.2 Volume row [wire session 3b + the 2026-11-10
    /// round trip]). Left/Right step the stops while the well is focused
    /// ("volume_selector" key context — bindings in main); the DEVISION
    /// dim (busy → 0.6) intentionally NOT applied (README).
    fn volume_panel(&self, cx: &Context<Self>) -> gpui::Div {
        let volume = &self.vm.volume;
        const VOLUME_STOPS: [&str; 6] = ["1", "2", "3", "4", "5", "6"];
        let group = segmented_selector(
            SelectorSpec {
                id_prefix: "volume-stop",
                labels: &VOLUME_STOPS,
                // The device's verified selection — no pending target is
                // ever shown (the snapshot's POST-SET poll moves it).
                is_selected: &|index| volume.selected == Some(index),
                // [`volume_pick`] is the whole decision (read-only family
                // or an in-flight volume write: blocked; already-selected:
                // noop; else write), shared by mouse AND the Left/Right
                // keys.
                is_pickable: &|index| volume_pick(index, volume) != VolumePick::Blocked,
                on_pick: |this, index, cx| this.set_gain_stop(index, cx),
                // The mock renders the six stops as one horizontal row
                // (index.html:111), NOT the NC rocker's vertical stack.
                horizontal: true,
                dim: volume.applying,
                focus: Some((&self.volume_focus, "volume_selector")),
            },
            cx,
        );
        // The reference keeps the Volume panel's own titled shell around the
        // radiogroup (`index.html:106-108` — the radio group lives INSIDE
        // the panel with the "Volume adjustment" heading), the same recipe
        // as the Channel/EQ/Schedule panels.
        panel()
            .flex_1()
            .justify_center()
            .gap(px(theme::metrics::SPACE_3))
            .child(
                div()
                    .text_size(px(theme::text::TITLE_SIZE))
                    .font_weight(FontWeight::SEMIBOLD)
                    // [code index.html:108]
                    .child("Volume adjustment"),
            )
            .child(group)
    }

    /// Channel-mode panel — INTERACTIVE since the channel-mode round trip
    /// (2026-11-11): the two mode pads send `set_channel_mode` (wire
    /// values via `channel_mode_pad_wire`) through the same actor path as
    /// the other controls, gated by `channel_pick`. While a channel-mode
    /// write is in flight (`ChannelModeVm.applying`) the selector dims to
    /// 0.5 and both mouse and keyboard picks are ignored — the snapshot's
    /// POST-SET poll moves the selection (no pending target is ever
    /// shown; same rule as the EQ/Volume selectors). Pad order IS byte
    /// order (Mono, Stereo — mono = wire 0, stereo = wire 1 [code
    /// HLDevice6302Activity.java:1803-1805]; the 2026-11-11 round trip
    /// exercised BOTH values on the wire). STEREO is additionally gated on
    /// BOTH transmitters being verified connected (`stereo_ok`, the VM's
    /// [`holly_app::vm::stereo_allowed`] rule — owner 2026-11-12): a gated
    /// pad renders without its click affordance and Left/Right steps over
    /// it as a noop; the held mode byte still shows. Left/Right step the
    /// pads while the well is focused ("channel_selector" key
    /// context — bindings in main); the DEVISION dim (busy → 0.6)
    /// intentionally NOT applied (README).
    fn channel_panel(&self, cx: &Context<Self>) -> gpui::Div {
        let channel = &self.vm.channel_mode;
        const CHANNEL_MODES: [&str; 2] = ["Mono", "Stereo"]; // [code index.html:258]
        let group = segmented_selector(
            SelectorSpec {
                id_prefix: "channel-mode",
                labels: &CHANNEL_MODES,
                // The device's verified selection — no pending target is
                // ever shown (the snapshot's POST-SET poll moves it).
                is_selected: &|index| channel.selected == Some(index),
                // [`channel_pick`] is the whole decision (read-only family
                // or an in-flight channel write: blocked; the held mode: noop;
                // STEREO additionally gated on BOTH transmitters verified
                // connected — `stereo_allowed`, owner 2026-11-12), shared by
                // mouse AND the Left/Right keys.
                is_pickable: &|index| channel_pick(index, channel) != ChannelPick::Blocked,
                on_pick: |this, index, cx| this.set_channel_mode(index, cx),
                // The mock renders the two mode pads as one horizontal row
                // (index.html:121-123), NOT the NC rocker's vertical stack.
                horizontal: true,
                dim: channel.applying,
                focus: Some((&self.channel_focus, "channel_selector")),
            },
            cx,
        );
        // The read-only audio_panel rendered this same shell (panel +
        // title + selector); the interactivity wraps in it unchanged.
        panel()
            .flex_1()
            .justify_center()
            .gap(px(theme::metrics::SPACE_3))
            .child(
                div()
                    .text_size(px(theme::text::TITLE_SIZE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Channel mode"),
            )
            .child(group)
    }

    /// The Schedule power-off panel — INTERACTIVE. Schedule maps
    /// quarter = 0 / never = 1 with the identity `shutdown_pad_wire` [code
    /// `HLDevice6302Activity.java:1864-1866`]; the wire values are the
    /// panel's own labels in order (15min, Never — index.html:264). While
    /// a shutdown-timer write is in flight (`ScheduleVm.applying`) the
    /// selector dims to 0.5 and both mouse and keyboard picks are ignored
    /// — the snapshot's POST-SET poll moves the selection (no pending
    /// target is ever shown; same rule as the EQ/Volume/channel
    /// selectors). Pad order IS byte order (the write caller
    /// `HLDevice6302Activity.java:1667-1671/:1672-1676` == the read
    /// renderer `:1864-1866` — identity, exercised BOTH values on the
    /// wire by the 2026-11-12 round trip). A byte outside the panel's
    /// certified set passes None and renders the no-selection rule.
    /// Left/Right step the pads while the well is focused
    /// ("schedule_selector" key context — bindings in main); the
    /// DEVISION dim (busy → 0.6) intentionally NOT applied (README).
    fn schedule_panel(&self, cx: &Context<Self>) -> gpui::Div {
        let schedule = &self.vm.shutdown;
        const SCHEDULE_VALUES: [&str; 2] = ["15min", "Never"]; // [code index.html:264]
        let group = segmented_selector(
            SelectorSpec {
                id_prefix: "schedule",
                labels: &SCHEDULE_VALUES,
                // The device's verified selection (a byte outside the
                // panel's certified set passes None → no-selection rule) —
                // no pending target is ever shown (the snapshot's POST-SET
                // poll moves it).
                is_selected: &|index| schedule.selected == Some(index),
                // [`schedule_pick`] is the whole decision (read-only family
                // or an in-flight shutdown write: blocked; already-selected:
                // noop; else write), shared by mouse AND the Left/Right keys.
                is_pickable: &|index| schedule_pick(index, schedule) != SchedulePick::Blocked,
                on_pick: |this, index, cx| this.set_schedule_power_off(index, cx),
                // The mock renders the two timer pads as one horizontal row
                // (index.html:151-153), NOT the NC rocker's vertical stack.
                horizontal: true,
                dim: schedule.applying,
                focus: Some((&self.schedule_focus, "schedule_selector")),
            },
            cx,
        );
        // The read-only audio_panel rendered this same shell (panel +
        // title + selector); the interactivity wraps in it unchanged.
        panel()
            .flex_1()
            .justify_center()
            .gap(px(theme::metrics::SPACE_3))
            .child(
                div()
                    .text_size(px(theme::text::TITLE_SIZE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Schedule power off"),
            )
            .child(group)
    }

    /// Reverb panel — INTERACTIVE since the reverb round trip (2026-11-11):
    /// the switch flips through `set_reverb` (opposite of the VERIFIED
    /// state, the device's current gear riding along — `reverb_toggle_wire`)
    /// and the size pads send (switch on, that gear) through the same actor
    /// path as the other controls, gated by `reverb_toggle_accepted`
    /// /`reverb_pick`. While a reverb write is in flight
    /// (`ReverbVm.applying`) BOTH controls dim to 0.5 (the established dim)
    /// and all picks are ignored — the snapshot's POST-SET poll moves
    /// everything (no pending target is ever shown). The selector keeps the
    /// read-only radio_group recipe byte-for-byte (row well, 36px pads,
    /// matte selected pad + tungsten bar) — only behaviour is added; the
    /// focus outline reuses the rocker's 2px tungsten rule.
    fn reverb_panel(&self, cx: &Context<Self>) -> gpui::Div {
        let reverb = self.vm.reverb.as_ref();
        let enabled = reverb.and_then(|r| r.enabled);
        let applying = reverb.is_some_and(|r| r.applying);
        panel()
            .flex_1()
            .justify_center()
            .gap(px(theme::metrics::SPACE_3))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(theme::text::TITLE_SIZE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Reverb adjustment"),
                    )
                    .child(self.reverb_toggle(enabled, applying, cx)),
            )
            .children((enabled == Some(true)).then(|| self.reverb_selector(applying, cx)))
    }

    /// The panel's switch: the read-only `toggle_switch` pill in the shared
    /// [`toggle_shell`] ("reverb_toggle" context + the rocker's 2px
    /// tungsten focus outline) and — ONLY while the flip may go out — a
    /// click listener.
    fn reverb_toggle(
        &self,
        enabled: Option<bool>,
        applying: bool,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let reverb = self.vm.reverb.as_ref();
        toggle_shell(
            "reverb-toggle",
            (&self.reverb_toggle_focus, "reverb_toggle"),
            applying,
            enabled == Some(true),
            reverb.is_some_and(reverb_toggle_accepted),
            |this| this.flip_reverb(),
            cx,
        )
    }

    /// The panel's size selector: the read-only radio_group recipe
    /// (radio_group + the APK's gear strings, selected from the certified
    /// byte) with the interactive shell added on top — track_focus +
    /// "reverb_selector" context + the rocker's 2px tungsten focus outline,
    /// per-pad click listeners (gated by `reverb_pick`), and the 0.5 dim
    /// while a reverb write is in flight. Shown only while the switch is
    /// verified ON (reference `sc-if reverbOn`, index.html:139).
    fn reverb_selector(&self, applying: bool, cx: &Context<Self>) -> gpui::Div {
        let reverb = self.vm.reverb.as_ref();
        const REVERB_GEARS: [&str; 3] = ["Small", "Medium", "Large"]; // [code index.html:140]
        segmented_selector(
            SelectorSpec {
                id_prefix: "reverb-gear",
                labels: &REVERB_GEARS,
                // The device's verified gear drives the selection — no
                // pending target is ever shown.
                is_selected: &|index| {
                    reverb.and_then(|r| r.gear).and_then(reverb_selection) == Some(index)
                },
                // [`reverb_pick`] is the whole decision (switch not verified
                // ON, the read-only family or an in-flight reverb write:
                // blocked; the device's own gear: noop; else write), shared
                // by mouse AND the Left/Right keys.
                is_pickable: &|index| {
                    reverb.is_some_and(|r| reverb_pick(index, r) != ReverbPick::Blocked)
                },
                on_pick: |this, index, _| this.set_reverb_gear(index),
                horizontal: true,
                dim: applying,
                focus: Some((&self.reverb_focus, "reverb_selector")),
            },
            cx,
        )
    }

    /// Mic recognition / Indicator light panels (the 120px row halves): the
    /// reference's one-row layout (`align-items:center;
    /// justify-content:space-between`, index.html:158). BOTH switches
    /// arrive prebuilt from their keyboard shells ([`Self::mic_toggle`] /
    /// [`Self::light_toggle`]): the indicator light is the wire-read
    /// `light` byte (§5 offset 11, inverted) and is INTERACTIVE since the
    /// light round trip (2026-11-12); mic recognition is the wire-read
    /// identify byte (§5 offset 15) and is INTERACTIVE since its own
    /// round trip (2026-11-12) — additionally gated on BOTH transmitters
    /// being verified connected (the APK's own precondition,
    /// HLDevice6302Activity.java:805-812).
    fn led_toggle_panel(&self, title: &'static str, switch: gpui::AnyElement) -> gpui::Div {
        panel()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(theme::text::TITLE_SIZE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(switch)
    }

    /// The Indicator light panel's switch: the read-only `toggle_switch`
    /// pill in the shared [`toggle_shell`] ("light_toggle" context) — ONLY
    /// while the flip may go out ([`light_toggle_accepted`]) a click
    /// listener; the established 0.5 dim rides while `LightVm.applying`
    /// (same recipe as [`Self::reverb_toggle`]).
    fn light_toggle(&self, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let light = &self.vm.light;
        toggle_shell(
            "light-toggle",
            (&self.light_toggle_focus, "light_toggle"),
            light.applying,
            light.on == Some(true),
            light_toggle_accepted(light),
            |this| this.flip_light(),
            cx,
        )
    }

    /// The Mic-recognition panel's switch: the read-only `toggle_switch`
    /// pill in the shared [`toggle_shell`] ("mic_toggle" context) — ONLY
    /// while the flip may go out ([`mic_toggle_accepted`], the family gate
    /// plus BOTH mics connected) a click listener; the established 0.5 dim
    /// rides while `MicRecognitionVm.applying` (same recipe as
    /// [`Self::light_toggle`]).
    fn mic_toggle(&self, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let mic = &self.vm.mic_recognition;
        toggle_shell(
            "mic-toggle",
            (&self.mic_toggle_focus, "mic_toggle"),
            mic.applying,
            mic.on == Some(true),
            mic_toggle_accepted(&self.vm),
            |this| this.flip_mic_recognition(),
            cx,
        )
    }

    // ---- footer (32px) --------------------------------------------------------

    /// Reference footer: 32px, top rule LINE_SOFT, 20px side padding,
    /// caption-size TEXT_3; LED + the short connection word
    /// (`conn_status_word`); right: the certified VID:PID (VM const). The
    /// DEMO marker rides next to the word — never hidden (mock-data rule).
    pub(crate) fn render_footer(&self) -> gpui::Div {
        div()
            .h(px(theme::metrics::FOOTER_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(theme::metrics::SPACE_2))
            .px(px(theme::metrics::SPACE_5))
            .border_t_1()
            .border_color(solid(theme::color::LINE_SOFT))
            .text_size(px(theme::text::CAPTION_SIZE))
            .text_color(solid(theme::color::TEXT_3))
            .child(led_dot(vm_led(&self.vm)))
            .child(div().child(conn_status_word(&self.vm)))
            .children(self.demo.map(|_| {
                div()
                    .text_color(solid(theme::color::TUNGSTEN))
                    .child("· demo")
            }))
            .child(div().ml_auto().child(VID_PID.to_string()))
    }
}

/// Mic card names: the design fixes exactly two receivers (Mic 1/Mic 2 —
/// heart, with "--"/"No data"). NOT device protocol data.
pub(crate) fn name_for(id: &str) -> &'static str {
    match id {
        "mic-1" => "Mic 1",
        _ => "Mic 2",
    }
}

/// The reference battery block: 64px/300 digits + (live) 20px "%" suffix +
/// right "Battery" label; 5 segments h8 r3 gap 4. `lit` segments take the
/// TEXT white fill (the device's real charge via `battery_segment_count`);
/// the rest stay the unlit RAISED fill. DEVIATION from the mock, which
/// lit all five on any live link regardless of percent (index.html:229) —
/// kept proportional so the bar agrees with the digits (README).
pub(crate) fn battery_block(
    digits: &str,
    live: bool,
    lit: usize,
    class: Option<BatteryClass>,
) -> gpui::Div {
    // The class tint (owner 2026-11-12), the APK's colour roles mapped onto
    // the closed token set: Full/High → TEXT (the mock's lit segments,
    // index.html:229, ≈ the APK's white/normal role [code
    // BatteryClipView.java:173]), Low → TUNGSTEN, Critical → ALERT (≈ the
    // APK's red role [code ic_battery_red.xml #fa5151,
    // BatteryClipView.java:173]; ALERT #FF6B6B is the closed set's nearest
    // signal colour). The APK tints the BAR, not the number (the 6108's %
    // text keeps a static colour, `HLDevice6302Activity.java:1759-1761`),
    // so the digits keep the mock's live/dead rule (numColor,
    // index.html:228) and only the segments ride the class.
    let tint = class.map(|class| match class {
        BatteryClass::Full | BatteryClass::High => theme::color::TEXT,
        BatteryClass::Low => theme::color::TUNGSTEN,
        BatteryClass::Critical => theme::color::ALERT,
    });
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(theme::metrics::SPACE_1))
                .child(
                    div()
                        .text_size(px(theme::text::READOUT_SIZE))
                        .font_weight(FontWeight::LIGHT)
                        .text_color(solid(if live {
                            theme::color::TEXT
                        } else {
                            theme::color::TEXT_DISABLED
                        }))
                        .child(digits.to_string()),
                )
                .children(live.then(|| {
                    div()
                        .text_size(px(theme::text::READOUT_UNIT_SIZE))
                        .text_color(solid(theme::color::TEXT_2))
                        .child("%")
                }))
                .child(
                    div()
                        .ml_auto()
                        .text_size(px(theme::text::BODY_SIZE))
                        .text_color(solid(theme::color::TEXT_2))
                        .child("Battery"),
                ),
        )
        .child(
            // 5 segments, h8 r3, gap 4; `lit` of them in the CLASS colour
            // (Full/High → TEXT keeps today's/mock's look, Low → TUNGSTEN,
            // Critical → ALERT), the rest the unlit RAISED fill. Lit count
            // stays the honest per-charge rule (`battery_segment_count`).
            div().flex().gap(px(theme::metrics::SPACE_1)).children(
                (0..theme::metrics::BATTERY_SEGMENTS).map(|segment| {
                    div()
                        .flex_1()
                        .h(px(8.))
                        .rounded(px(theme::metrics::BATTERY_SEGMENT_RADIUS))
                        .bg(solid(if live && segment < lit {
                            tint.unwrap_or(theme::color::TEXT)
                        } else {
                            theme::color::RAISED
                        }))
                        .into_any_element()
                }),
            ),
        )
}

/// The reference level meter: a "Level" caption (13px TEXT_2) above a row of
/// 20 segments (row gap 3, height 14, radius 2; index.html:74-78). The VM
/// carries NO per-mic level field (HeartInfo exposes none we have decoded),
/// so every segment renders the unlit RAISED fill — always. This is the one
/// unfinished reproduction in the mock (README: deviations); a live meter
/// REQUIREMENT, not an invented device reading.
pub(crate) fn level_meter_block() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.)) // [code index.html:74]
        .child(
            div()
                .text_size(px(theme::text::BODY_SIZE))
                .text_color(solid(theme::color::TEXT_2))
                .child("Level"),
        )
        .child(
            div()
                .flex()
                .gap(px(3.)) // [code index.html:76]
                .children((0..theme::metrics::METER_SEGMENTS).map(|_| {
                    div()
                        .flex_1()
                        .h(px(theme::metrics::METER_SEGMENT_HEIGHT))
                        .rounded(px(theme::metrics::METER_SEGMENT_RADIUS))
                        .bg(solid(theme::color::RAISED))
                        .into_any_element()
                })),
        )
}
