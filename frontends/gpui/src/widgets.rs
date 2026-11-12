//! Reusable view pieces (pure move out of main.rs, commit A): the embedded
//! reference SVG icons and their in-memory `AssetSource`, the theme-recipe
//! helpers (`solid`, `matte_under_edge`, `panel`, `well`), the LED machinery
//! (`LedLit`, `led_dot`, `titled_with_led`) and the read-only
//! `toggle_switch` pill.
//!
//! Every gpui call's provenance is cited in the module docs of `main.rs`
//! (and inline below); nothing here is written from memory.

use std::borrow::Cow;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AssetSource, BoxShadow, ClickEvent, Context, ElementId, FocusHandle,
    FontWeight, SharedString, div, linear_color_stop, linear_gradient, point, prelude::*,
    pulsating_between, px, rgb, svg,
};

use crate::AppModel;
use crate::theme;

// ---- reference SVG markup (copied from docs/design/index.html) ---------------
// The `svg()` element renders an alpha mask tinted with ONE color, so each
// icon carries the reference geometry and gets its color from
// `.text_color(...)` on the svg element itself (svg.rs:113 zips
// `path` with `style.text.color` — nothing paints without it).

pub(crate) const ICON_MIC: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#ECEEF1" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/></svg>"##;

/// Banner icon (the reference's amber plug glyph, index.html:39).
pub(crate) const ICON_CHARGER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#ECEEF1" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M9 2v5M15 2v5M6 7h12v4a6 6 0 0 1-12 0V7zM12 17v5"/></svg>"##;

pub(crate) const ICON_CLOSE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#ECEEF1" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6 6l12 12M18 6L6 18"/></svg>"##;

/// In-memory `AssetSource`: the key IS the SVG source. `svg()` asks the
/// source for bytes under `.path(..)` (elements/svg.rs:120); returning the
/// key's own bytes keeps icons inline with zero filesystem and zero new
/// dependencies (trait method signatures per src/assets.rs:13-18).
pub(crate) struct IconAssets;

impl AssetSource for IconAssets {
    // `gpui::Result` IS `anyhow::Result` (re-export, src/gpui.rs:69), so this
    // impl needs no anyhow dependency of its own. The loader is keyed BY THE
    // SOURCE ITSELF: `svg_glyph` hands the full embedded SVG markup string to
    // `svg()`, which resolves it through this AssetSource (svg.rs renders by
    // the path key, assets.rs:13-18); matching against the consts restores
    // the 'static signature — the &'static bytes are returned, the borrowed
    // user/viewer copy never escapes. An unknown key loads as None (renders
    // blank rather than panicking).
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let src = match path {
            ICON_MIC => ICON_MIC,
            ICON_CHARGER => ICON_CHARGER,
            ICON_CLOSE => ICON_CLOSE,
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(src.as_bytes())))
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

/// One monochrome glyph: `svg()` + source + explicit color + square size
/// (the element picks its color from its own text style, svg.rs:113).
pub(crate) fn svg_glyph(source: &'static str, side: f32, color: gpui::Rgba) -> gpui::Svg {
    svg()
        .path(SharedString::from(source))
        .size(px(side))
        .text_color(color)
}

// ---- small view helpers over theme tokens ------------------------------------

/// A solid token as `Rgba` (theme.rs stores 0xRRGGBB).
pub(crate) fn solid(token: u32) -> gpui::Rgba {
    rgb(token)
}

/// The reference's non-inset drop shadows, mapped to the WELL token (`#0B0C0E`
/// → WELL, the nearest token; the token set is closed): the matte pads' hard
/// under-edge `0 2px 0 #0B0C0E` (index.html:95/234) and the selected selector
/// option's `0 1px 0 #000` line (index.html:202). The matching `inset 0 1px 0`
/// top highlights are NOT reproduced — gpui has no inset shadows (README:
/// unfinished reproductions).
pub(crate) fn matte_under_edge(offset_y: f32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: solid(theme::color::WELL).into(),
        offset: point(px(0.), px(offset_y)),
        blur_radius: px(0.),
        spread_radius: px(0.),
    }]
}

/// The panel recipe from docs/design/index.html: 20px padding, 14px radius,
/// 1px LINE_PANEL border, faceplate gradient, and the reference's
/// `0 1px 0 rgba(0,0,0,.5)` bottom edge painted as a BoxShadow (theme.rs
/// `DROP_DARK` recipe). The reference's `inset 0 1px 0` top highlight is
/// NOT reproduced — gpui has no inset shadows (README: unfinished
/// reproductions).
pub(crate) fn panel() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .p(px(theme::metrics::SPACE_5))
        .rounded(px(theme::metrics::PANEL_RADIUS))
        .border_1()
        .border_color(solid(theme::color::LINE_PANEL))
        .bg(linear_gradient(
            180.,
            linear_color_stop(solid(theme::color::PANEL_TOP), 0.),
            linear_color_stop(solid(theme::color::PANEL_BOTTOM), 1.),
        ))
        .shadow(vec![BoxShadow {
            color: gpui::Hsla::from(solid(0x000000)).opacity(theme::color::DROP_DARK.1),
            offset: point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
        }])
}

/// The recessed "well" recipe: WELL fill; the reference's inset shadows
/// (`inset 0 2px 8px rgba(0,0,0,.8)`, ...) fall back to a 1px LINE_SOFT
/// border (README: unfinished reproductions).
pub(crate) fn well(radius: f32) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(radius))
        .border_1()
        .border_color(solid(theme::color::LINE_SOFT))
        .bg(solid(theme::color::WELL))
}

/// A status LED dot: 7px circle; LIVE/TUNGSTEN/ALERT get the designed LED
/// glow (theme.rs `GLOW_*_LED`: blur 8); the ring state draws a
/// TEXT_DISABLED contour with no fill and no glow (reference: `1.5px solid
/// #5A616B` — nearest token, 1px, README).
/// LED states, decided ONLY from VM fields ([code] index.html:215-217).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LedLit {
    Live,
    /// Attention = tungsten; `pulse` reproduces the reference's
    /// `holly-pulse 1.1s` keyframes for the pending state (index.html:15).
    Attention {
        pulse: bool,
    },
    Alert,
    Ring,
}

impl LedLit {
    fn color(&self) -> gpui::Rgba {
        match self {
            Self::Live => solid(theme::color::LIVE),
            Self::Attention { .. } => solid(theme::color::TUNGSTEN),
            Self::Alert => solid(theme::color::ALERT),
            Self::Ring => solid(theme::color::TEXT_DISABLED),
        }
    }

    fn glow(&self) -> Option<gpui::Hsla> {
        match self {
            Self::Live => Some(
                gpui::Hsla::from(solid(theme::color::GLOW_LIVE_LED.0))
                    .opacity(theme::color::GLOW_LIVE_LED.1),
            ),
            Self::Attention { .. } => Some(
                gpui::Hsla::from(solid(theme::color::GLOW_TUNGSTEN_LED.0))
                    .opacity(theme::color::GLOW_TUNGSTEN_LED.1),
            ),
            Self::Alert => Some(
                // Same LED-glow alpha as the designed glows (0.70), on the
                // alert signal color — composition documented in README.
                gpui::Hsla::from(solid(theme::color::ALERT)).opacity(0.70),
            ),
            Self::Ring => None,
        }
    }
}
/// Render the LED dot; the pulsing state wraps the circle in the
/// reference's 1.1s breathing animation (`.repeat()` +
/// `pulsating_between(0.3, 1.0)` ≙ `0%,100%{opacity:1}50%{opacity:.3}`).
pub(crate) fn led_dot(lit: LedLit) -> gpui::AnyElement {
    let glow_dot = |color: gpui::Rgba, glow: gpui::Hsla| {
        div()
            .w(px(theme::metrics::LED_SIZE))
            .h(px(theme::metrics::LED_SIZE))
            .rounded_full()
            .bg(color)
            .shadow(vec![BoxShadow {
                color: glow,
                offset: point(px(0.), px(0.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
            }])
            .into_any_element()
    };
    let ring_dot = || {
        div()
            .w(px(theme::metrics::LED_SIZE))
            .h(px(theme::metrics::LED_SIZE))
            .rounded_full()
            .border_1()
            .border_color(solid(theme::color::TEXT_DISABLED))
            .into_any_element()
    };
    match &lit {
        LedLit::Ring => ring_dot(),
        LedLit::Alert => glow_dot(lit.color(), lit.glow().unwrap()),
        LedLit::Attention { pulse: false } => glow_dot(lit.color(), lit.glow().unwrap()),
        LedLit::Attention { pulse: true } => {
            let dot = div()
                .w(px(theme::metrics::LED_SIZE))
                .h(px(theme::metrics::LED_SIZE))
                .rounded_full()
                .bg(lit.color())
                .shadow(vec![BoxShadow {
                    color: lit.glow().unwrap(),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(8.),
                    spread_radius: px(0.),
                }]);
            dot.with_animation(
                "led-pulse",
                Animation::new(Duration::from_millis(1100))
                    .repeat()
                    .with_easing(pulsating_between(0.3, 1.0)),
                |dot, delta| dot.opacity(delta),
            )
            .into_any_element()
        }
        LedLit::Live => glow_dot(lit.color(), lit.glow().unwrap()),
    }
}

/// `title + led + status word` row used by the panels (title 15/600, the
/// status word 13/TEXT_2 next to its 7px LED).
pub(crate) fn titled_with_led(title: &'static str, lit: LedLit, status: String) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .text_size(px(theme::text::TITLE_SIZE))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_color(solid(theme::color::TEXT_2))
                .text_size(px(theme::text::BODY_SIZE))
                .child(led_dot(lit))
                .child(status),
        )
}

/// The small on/off switch (48×26 track, 20px thumb; reference `tog()`).
/// The pill itself adds NO interaction — it renders the wire-read state at
/// full opacity; an interactive caller wraps it (the reverb switch is the
/// only one today) and owns the click/keyboard wiring outside.
pub(crate) fn toggle_switch(on: bool) -> gpui::Div {
    let track = div()
        .relative()
        .w(px(theme::metrics::TOGGLE_WIDTH))
        .h(px(theme::metrics::TOGGLE_HEIGHT))
        // the mock's toggle track: border-radius 13px on 48×26
        // (index.html:137/160/164) — the TOGGLE_RADIUS token, a full round
        // for this geometry
        .rounded(px(theme::metrics::TOGGLE_RADIUS))
        .when(on, |t| {
            t.border_1()
                .border_color(solid(theme::color::TUNGSTEN_BOTTOM))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(solid(theme::color::TUNGSTEN_TOP), 0.),
                    linear_color_stop(solid(theme::color::TUNGSTEN_BOTTOM), 1.),
                ))
        })
        .when(!on, |t| {
            t.border_1()
                .border_color(solid(theme::color::LINE_SOFT))
                .bg(solid(theme::color::WELL))
        });
    track.child(
        div()
            .absolute()
            .top(px(3.))
            .left(px(if on {
                theme::metrics::TOGGLE_WIDTH - theme::metrics::TOGGLE_THUMB - 3.
            } else {
                3.
            }))
            .w(px(theme::metrics::TOGGLE_THUMB))
            .h(px(theme::metrics::TOGGLE_THUMB))
            .rounded_full()
            .shadow(vec![BoxShadow {
                color: gpui::Hsla::from(solid(0x000000)).opacity(0.5),
                offset: point(px(0.), px(1.)),
                blur_radius: px(2.),
                spread_radius: px(0.),
            }])
            .bg(solid(if on {
                theme::color::TEXT
            } else {
                theme::color::TEXT_2
            })),
    )
}

/// The keyboard/click shell around a [`toggle_switch`] pill — the reverb /
/// Indicator-light / Mic-recognition panels' switches: an id'd, focusable
/// wrapper (the rocker's 2px tungsten focus outline, `key_context` scoped)
/// that adds NO padding (the pill's own footprint, so the row's
/// space-between keeps the toggle where the read-only render had it), dims
/// to the established 0.5 while the control's OWN write is in flight, and —
/// ONLY while the flip may go out (`accepted`, the caller's
/// `controls::*_toggle_accepted` gate) — carries the click listener.
/// `focus` is the keyboard scope (handle + key_context), the same shape as
/// [`SelectorSpec::focus`] minus the None case. `on_flip` is a plain fn
/// pointer for the same 'static listener bound as
/// [`segmented_selector`]'s `on_pick` (gpui `Context::listener`,
/// context.rs:252); the non-capturing call-site closures coerce to it.
pub(crate) fn toggle_shell(
    id: &'static str,
    focus: (&FocusHandle, &'static str),
    dim: bool,
    on: bool,
    accepted: bool,
    on_flip: fn(&mut AppModel),
    cx: &Context<AppModel>,
) -> gpui::Stateful<gpui::Div> {
    let mut shell = div()
        .id(id)
        .rounded(px(theme::metrics::TOGGLE_RADIUS))
        .track_focus(focus.0)
        .key_context(focus.1)
        .focus(|s| s.border_2().border_color(solid(theme::color::TUNGSTEN)))
        .when(dim, |t| t.opacity(0.5))
        .child(toggle_switch(on));
    if accepted {
        shell = shell.on_click(cx.listener(move |this, _: &ClickEvent, _, _| on_flip(this)));
    }
    shell
}

// ---- the segmented multi-choice selector ---------------------------------------

/// One segmented selector's wiring, stated once per control (the only
/// differences the six selectors really have — see the per-selector docs in
/// panels.rs for their [code]/[wire] cites):
/// - `labels`/`id_prefix` — the option row texts and the element-id prefix;
/// - `is_selected` — the device's VERIFIED selection (VM fields only; no
///   pending target is ever displayed);
/// - `is_pickable` + `on_pick` — the caller's pick decision (its
///   `controls::*_pick != Blocked`) and the write it sends;
/// - `horizontal` — the reverb row shape (well `flex_row`, 36px pads,
///   indicator bar 4px up) vs the NC-shaped vertical stack (well column,
///   `flex_1`/`min_h(0)` pads, bar 5px up). TODAY: NC/EQ/Volume/Channel/
///   Schedule render vertical, reverb horizontal;
/// - `dim` — the family's 0.5 dim while ITS write is in flight;
/// - `focus` — the well's keyboard scope (`track_focus` + `key_context` +\
///   the rocker's 2px tungsten focus outline); None for the NC level
///   selector, whose keys are the global 1/2/3 (its rocker owns its own
///   context).
///
/// The predicates borrow the panel's VM refs and are consumed during THIS
/// render, so they stay closures; `on_pick` is a plain `fn` pointer because
/// the click listener is stored in the element tree and must be 'static
/// (gpui `Context::listener` bound, context.rs:252) — a closure borrowing
/// `&self.vm.*` would not be. Non-capturing closures at the call sites
/// coerce to the pointer.
pub(crate) struct SelectorSpec<'a> {
    pub id_prefix: &'static str,
    pub labels: &'a [&'static str],
    pub is_selected: &'a dyn Fn(usize) -> bool,
    pub is_pickable: &'a dyn Fn(usize) -> bool,
    pub on_pick: fn(&mut AppModel, usize, &mut Context<AppModel>),
    pub horizontal: bool,
    pub dim: bool,
    pub focus: Option<(&'a FocusHandle, &'static str)>,
}

/// The shared segmented selector: the WELL container (12 radius, 4 padding,
/// 2 gap), one 36px/matte option per label with the selected option's
/// RAISED gradient + SEMIBOLD + matte under-edge, and the 14×2 tungsten
/// indicator bar under it — NC selector recipe (index.html:100, bar 5px up)
/// vertically, the read-only radio_group recipe (index.html:111, bar 4px
/// up) horizontally. While `dim` holds the group renders at the established
/// 0.5 and a blocked pick's option carries NO click affordance (mouse AND
/// keyboard noop — the decisions live in the caller's pick fn). This is the
/// ONE place the option loop, selected style and bar exist; the six panel
/// selectors in panels.rs all route through it.
pub(crate) fn segmented_selector(spec: SelectorSpec, cx: &Context<AppModel>) -> gpui::Div {
    let mut group = well(theme::metrics::SELECTOR_RADIUS_OUTER);
    group = if spec.horizontal {
        group
            .flex_row()
            .p(px(theme::metrics::SELECTOR_PADDING))
            .gap(px(theme::metrics::SELECTOR_GAP))
    } else {
        group
            .flex_1()
            .min_w(px(0.))
            .p(px(theme::metrics::SELECTOR_PADDING))
            .gap(px(theme::metrics::SELECTOR_GAP))
    };
    if let Some((focus, context)) = spec.focus {
        group = group
            .track_focus(focus)
            .key_context(context)
            // focus-visible outline = the rocker's rule (2px tungsten).
            .focus(|s| s.border_2().border_color(solid(theme::color::TUNGSTEN)));
    }
    if spec.dim {
        group = group.opacity(0.5);
    }
    // The indicator bar rides 5px above the pad bottom in the vertical
    // family (index.html:100) and 4px in the horizontal one (index.html:111);
    // glow `0 0 6px rgba(255,181,71,.6)` (index.html:100/111).
    let bar_bottom = if spec.horizontal { 4. } else { 5. };
    // `on_pick` is a fn pointer ('static): the click listener below outlives
    // this render, so only Copy 'static values may be moved into it.
    let on_pick = spec.on_pick;
    for (index, label) in spec.labels.iter().enumerate() {
        let selected = (spec.is_selected)(index);
        let base = div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::metrics::SELECTOR_RADIUS_INNER))
            .text_size(px(theme::text::BODY_SIZE))
            .when(selected, |o| {
                o.bg(linear_gradient(
                    180.,
                    linear_color_stop(solid(theme::color::RAISED_TOP), 0.),
                    linear_color_stop(solid(theme::color::RAISED_BOTTOM), 1.),
                ))
                .text_color(solid(theme::color::TEXT))
                .font_weight(FontWeight::SEMIBOLD)
                .shadow(matte_under_edge(1.))
            })
            .when(!selected, |o| {
                o.text_color(solid(theme::color::TEXT_2))
                    .font_weight(FontWeight::MEDIUM)
            })
            .child((*label).to_string())
            .relative();
        // The two shapes' option sizing, verbatim from today's code: the
        // vertical family grows by flex_1/min_h(0) (nc_selector), the
        // horizontal reverb recipe pins min_w(0) + the 36px SELECTOR_HEIGHT
        // (reverb_selector, index.html:111).
        let mut option = if spec.horizontal {
            base.min_w(px(0.)).h(px(theme::metrics::SELECTOR_HEIGHT))
        } else {
            base.min_h(px(0.))
        };
        if selected {
            option = option.child(
                div()
                    .absolute()
                    .left(px(0.))
                    .right(px(0.))
                    .bottom(px(bar_bottom))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w(px(theme::metrics::LEVEL_INDICATOR_WIDTH))
                            .h(px(theme::metrics::LEVEL_INDICATOR_HEIGHT))
                            .rounded(px(1.))
                            .bg(solid(theme::color::TUNGSTEN))
                            .shadow(vec![BoxShadow {
                                color: gpui::Hsla::from(solid(theme::color::TUNGSTEN)).opacity(0.6),
                                offset: point(px(0.), px(0.)),
                                blur_radius: px(6.),
                                spread_radius: px(0.),
                            }]),
                    ),
            );
        }
        let option = if (spec.is_pickable)(index) {
            option
                .id(ElementId::named_usize(spec.id_prefix, index))
                .hover(|s| s.cursor_pointer())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    on_pick(this, index, cx);
                }))
                .into_any_element()
        } else {
            option.into_any_element()
        };
        group = group.child(option);
    }
    group
}
