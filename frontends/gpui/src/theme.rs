//! Holly theme tokens: "graphite and tungsten".
//!
//! Pure data, no gpui dependency, so it compiles and tests anywhere.
//! Colours are 0xRRGGBB, ready for `gpui::rgb(..)`. Translucent colours are
//! (0xRRGGBB, alpha) pairs; convert them in the frontend with whatever alpha
//! helper the pinned gpui version provides (verify it, do not assume).
//!
//! Colour rule: amber (tungsten) means "engaged or needs attention", green means
//! a live connection, red means an error. Everything else stays grey.
//! Visual reference: docs/design/index.html (states) and docs/design/tokens.html.
//!
//! T1 note: the token surface is complete BY DESIGN while the view layer is
//! still the pre-T3 one; the constants not yet consumed emit dead-code
//! warnings, so this file carries one scoped allow. It is dissolved in T3,
//! when every view consumes tokens from here.
#![allow(dead_code)]

pub mod color {
    // Surfaces
    pub const CHASSIS_TOP: u32 = 0x15181C; // window gradient, top
    pub const CHASSIS: u32 = 0x0F1114; // window gradient, bottom
    pub const PANEL_TOP: u32 = 0x1C1F24; // faceplate gradient, top
    pub const PANEL_BOTTOM: u32 = 0x15171B; // faceplate gradient, bottom
    pub const WELL: u32 = 0x0A0B0D; // recessed areas (rocker well, mic badge)
    pub const RAISED: u32 = 0x262A30; // buttons, unlit segments
    pub const RAISED_TOP: u32 = 0x2B2F36; // matte pad gradient, top
    pub const RAISED_BOTTOM: u32 = 0x22252B; // matte pad gradient, bottom
    pub const LINE: u32 = 0x2A2E35; // window border
    pub const LINE_PANEL: u32 = 0x262A30; // panel border
    pub const LINE_SOFT: u32 = 0x22262C; // titlebar and footer rules

    // Text
    pub const TEXT: u32 = 0xECEEF1;
    pub const TEXT_2: u32 = 0xA3AAB4;
    pub const TEXT_3: u32 = 0x7F8793;
    pub const TEXT_DISABLED: u32 = 0x5A616B; // decorative or disabled only, below 4.5:1 by design

    // Signal
    pub const TUNGSTEN: u32 = 0xFFB547;
    pub const TUNGSTEN_TOP: u32 = 0xFFC66E; // lit pad gradient, top
    pub const TUNGSTEN_BOTTOM: u32 = 0xF5A524; // lit pad gradient, bottom
    pub const INK: u32 = 0x1A1205; // text on tungsten
    pub const LIVE: u32 = 0x5FD38D;
    pub const ALERT: u32 = 0xFF6B6B;

    // Attention banner (receiver busy, permission denied)
    pub const BANNER_BG: u32 = 0x221C12;
    pub const BANNER_LINE: u32 = 0x3A2E18;
    pub const BANNER_TEXT: u32 = 0xC9B38A;

    // Translucent recipes: (rgb, alpha)
    pub const HIGHLIGHT_TOP: (u32, f32) = (0xFFFFFF, 0.06); // 1px top edge of panels
    pub const HIGHLIGHT_PAD: (u32, f32) = (0xFFFFFF, 0.08); // top edge of matte pads
    pub const HIGHLIGHT_LIT: (u32, f32) = (0xFFFFFF, 0.50); // top edge of lit pad
    pub const DROP_DARK: (u32, f32) = (0x000000, 0.50); // 1px line under panels
    pub const GLOW_TUNGSTEN: (u32, f32) = (0xFFB547, 0.35); // lit pad glow, blur 32
    pub const GLOW_TUNGSTEN_LED: (u32, f32) = (0xFFB547, 0.70); // LED glow, blur 8
    pub const GLOW_LIVE_LED: (u32, f32) = (0x5FD38D, 0.70); // LED glow, blur 8
}

/// Sizes in logical pixels.
pub mod metrics {
    pub const WINDOW_RADIUS: f32 = 12.0;
    pub const PANEL_RADIUS: f32 = 14.0;
    pub const WELL_RADIUS: f32 = 26.0;
    pub const PAD_RADIUS_OUTER: f32 = 14.0;
    pub const PAD_RADIUS_INNER: f32 = 6.0;
    pub const BUTTON_RADIUS: f32 = 8.0;
    pub const BATTERY_SEGMENT_RADIUS: f32 = 3.0;
    pub const METER_SEGMENT_RADIUS: f32 = 2.0;

    pub const SPACE_1: f32 = 4.0;
    pub const SPACE_2: f32 = 8.0;
    pub const SPACE_3: f32 = 12.0;
    pub const SPACE_4: f32 = 16.0;
    pub const SPACE_5: f32 = 20.0;

    pub const TITLEBAR_HEIGHT: f32 = 44.0;
    pub const FOOTER_HEIGHT: f32 = 32.0;
    pub const WINDOW_DEFAULT_WIDTH: f32 = 1080.0; // also the minimum size
    pub const WINDOW_DEFAULT_HEIGHT: f32 = 760.0;
    pub const MIC_COLUMN_WIDTH: f32 = 336.0; // left column: Mic 1 and Mic 2 cards
    pub const GRID_ROW_EQ_HEIGHT: f32 = 132.0; // EQ adjustment and reverb adjustment row
    pub const GRID_ROW_POWER_HEIGHT: f32 = 120.0; // schedule, mic recognition, indicator row
    pub const ROCKER_WIDTH: f32 = 116.0;
    pub const ROCKER_PAD_ON_HEIGHT: f32 = 72.0;
    pub const ROCKER_PAD_OFF_HEIGHT: f32 = 64.0;
    pub const TOGGLE_WIDTH: f32 = 48.0; // small on/off switch
    pub const TOGGLE_HEIGHT: f32 = 26.0;
    pub const TOGGLE_RADIUS: f32 = 13.0;
    pub const TOGGLE_THUMB: f32 = 20.0;
    pub const METER_SEGMENTS: usize = 20; // live mic level
    pub const METER_SEGMENT_HEIGHT: f32 = 14.0;
    pub const METER_PEAK_FROM: usize = 17; // segments from here up use TUNGSTEN
    pub const SELECTOR_HEIGHT: f32 = 36.0; // NC level selector segment
    pub const SELECTOR_PADDING: f32 = 4.0;
    pub const SELECTOR_GAP: f32 = 2.0;
    pub const SELECTOR_RADIUS_OUTER: f32 = 12.0;
    pub const SELECTOR_RADIUS_INNER: f32 = 8.0;
    pub const LEVEL_INDICATOR_WIDTH: f32 = 14.0; // tungsten bar under the selected level
    pub const LEVEL_INDICATOR_HEIGHT: f32 = 2.0;
    pub const LED_SIZE: f32 = 7.0;
    pub const BATTERY_SEGMENTS: usize = 5;
}

/// Type scale. Family: Hanken Grotesk (OFL), bundled with the app; fall back to
/// the system sans if loading fails. Use tabular figures for readouts if the
/// pinned gpui exposes font features (verify).
pub mod text {
    pub const FAMILY: &str = "Hanken Grotesk";
    pub const READOUT_SIZE: f32 = 64.0; // weight 300
    pub const READOUT_UNIT_SIZE: f32 = 20.0;
    pub const TITLE_SIZE: f32 = 15.0; // weight 600
    pub const BODY_SIZE: f32 = 13.0; // weight 400
    pub const CAPTION_SIZE: f32 = 12.0; // weight 400
    pub const WEIGHT_LIGHT: u16 = 300;
    pub const WEIGHT_REGULAR: u16 = 400;
    pub const WEIGHT_SEMIBOLD: u16 = 600;
}

/// WCAG 2.x contrast ratio between two 0xRRGGBB colours.
pub fn contrast(a: u32, b: u32) -> f64 {
    fn lin(c: u32) -> f64 {
        let c = c as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    fn lum(rgb: u32) -> f64 {
        0.2126 * lin((rgb >> 16) & 0xFF)
            + 0.7152 * lin((rgb >> 8) & 0xFF)
            + 0.0722 * lin(rgb & 0xFF)
    }
    let (la, lb) = (lum(a), lum(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::color::*;
    use super::contrast;

    fn at_least(fg: u32, bg: u32, min: f64, what: &str) {
        let r = contrast(fg, bg);
        assert!(
            r >= min,
            "{what}: {fg:06X} on {bg:06X} is {r:.2}:1, needs {min}:1"
        );
    }

    #[test]
    fn body_text_meets_aa_on_every_surface() {
        for (bg, name) in [
            (CHASSIS, "chassis"),
            (PANEL_TOP, "panel top"),
            (PANEL_BOTTOM, "panel bottom"),
        ] {
            at_least(TEXT, bg, 4.5, &format!("primary on {name}"));
            at_least(TEXT_2, bg, 4.5, &format!("secondary on {name}"));
            at_least(TEXT_3, bg, 4.5, &format!("tertiary on {name}"));
        }
    }

    #[test]
    fn controls_meet_aa() {
        at_least(INK, TUNGSTEN_TOP, 4.5, "ink on lit pad top");
        at_least(INK, TUNGSTEN_BOTTOM, 4.5, "ink on lit pad bottom");
        at_least(TEXT_2, RAISED_TOP, 4.5, "label on matte pad top");
        at_least(TEXT_2, RAISED_BOTTOM, 4.5, "label on matte pad bottom");
        at_least(TEXT, RAISED, 4.5, "button label");
        at_least(TEXT, BANNER_BG, 4.5, "banner title");
        at_least(BANNER_TEXT, BANNER_BG, 4.5, "banner body");
    }

    #[test]
    fn non_text_indicators_meet_3_to_1() {
        at_least(TUNGSTEN, WELL, 3.0, "tungsten on well");
        at_least(TUNGSTEN, PANEL_BOTTOM, 3.0, "tungsten LED on panel");
        at_least(LIVE, PANEL_BOTTOM, 3.0, "live LED on panel");
        at_least(ALERT, PANEL_BOTTOM, 3.0, "alert on panel");
        at_least(TEXT_2, RAISED, 3.0, "lit level segment vs unlit segment");
    }
}
