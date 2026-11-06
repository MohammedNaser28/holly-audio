//! Bundled fonts (T2): the design reference's Hanken Grotesk faces.
//!
//! `theme.rs` (`text::FAMILY`) names the face; this module registers the
//! byte blobs BEFORE the first window opens, so glyph resolution finds them.
//! Loading an already-registered set again would only re-parse; `register`
//! is called exactly once from `main`.
//!
//! Containers are plain sfnt (ttf), converted once from the reference's
//! woff2 files with system fontTools — an asset step, not a build
//! dependency. Reason [verified, no assumption]: gpui 0.2.2's Linux font
//! registry feeds bytes into cosmic-text's fontdb
//! (`platform/linux/text_system.rs:196-204`, `fontdb::Database::load_font_data`),
//! and fontdb/ttf-parser parse sfnt containers only — the woff2 wrapper
//! cannot be registered, so the faces ship expanded. Same glyphs, same
//! family/weight axes (verified: family "Hanken Grotesk", usWeightClass
//! 300/400/500/600; digit advances equal — readouts render tabular even
//! though the latin subset carries no `tnum` feature).
//!
//! Fallback rule (design brief): if registration fails at runtime, log ONCE
//! and keep the system sans — never retry per frame, never abort the app
//! over an asset.

use std::borrow::Cow;

use gpui::App;

/// Registered faces, weight order 300/400/500/600 (each embeds compile-time,
/// so a missing asset fails the build, not the first render).
const FACES: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/hanken-grotesk-latin-300-normal.ttf"),
    include_bytes!("../assets/fonts/hanken-grotesk-latin-400-normal.ttf"),
    include_bytes!("../assets/fonts/hanken-grotesk-latin-500-normal.ttf"),
    include_bytes!("../assets/fonts/hanken-grotesk-latin-600-normal.ttf"),
];

/// Register the bundled faces with the text system. Errors are logged once
/// (stderr) and swallowed: the app then resolves `theme::text::FAMILY`
/// against the system sans (unknown family) instead of aborting.
pub fn register(cx: &App) {
    if let Err(e) = cx
        .text_system()
        .add_fonts(FACES.iter().copied().map(Cow::Borrowed).collect())
    {
        eprintln!(
            "holly-gpui: bundled fonts failed to load ({e}); falling back to the system sans"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::FACES;
    use crate::theme::text;

    /// The family the views render must be exactly what the bundled faces
    /// carry (verified against the converted files' name tables); otherwise
    /// resolution silently degrades to the system sans.
    #[test]
    fn family_constant_matches_the_bundled_faces() {
        assert_eq!(text::FAMILY, "Hanken Grotesk");
    }

    /// Every bundled blob must BE an sfnt 1.0 font (what fontdb parses);
    /// a stray woff2 would pass `include_bytes!` and fail only at runtime.
    #[test]
    fn bundled_faces_are_sfnt() {
        for face in FACES {
            assert_eq!(&face[..4], &[0, 1, 0, 0], "sfnt 1.0 magic");
        }
    }
}
