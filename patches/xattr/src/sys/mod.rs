macro_rules! platforms {
    ($($($platform:expr);* => $module:ident),*) => {
        $(
            #[cfg(any($(target_os = $platform),*))]
            #[cfg_attr(not(any($(target_os = $platform),*)), allow(dead_code))]
            mod $module;

            #[cfg(any($(target_os = $platform),*))]
            pub use self::$module::*;

            // PATCHED (see comment above the `platforms!` invocation):
            // libc no longer defines ENOATTR on linux/android; the same
            // errno (61) is spelled ENODATA there.
            #[cfg(all(any($(target_os = $platform),*), not(any(target_os = "linux", target_os = "android"))))]
            pub const ENOATTR: ::libc::c_int = ::libc::ENOATTR;
            #[cfg(all(any($(target_os = $platform),*), any(target_os = "linux", target_os = "android")))]
            pub const ENOATTR: ::libc::c_int = ::libc::ENODATA;
        )*

        #[cfg(all(feature = "unsupported", not(any($($(target_os = $platform),*),*))))]
        #[cfg_attr(any($($(target_os = $platform),*),*), allow(dead_code))]
        mod unsupported;

        #[cfg(all(feature = "unsupported", not(any($($(target_os = $platform),*),*))))]
        pub use self::unsupported::*;
        #[cfg(all(feature = "unsupported", not(any($($(target_os = $platform),*),*))))]
        pub const ENOATTR: ::libc::c_int = 0;


        /// A constant indicating whether or not the target platform is supported.
        ///
        /// To make programmer's lives easier, this library builds on all platforms.
        /// However, all function calls on unsupported platforms will return
        /// `io::Error`s.
        ///
        /// Note: If you would like compilation to simply fail on unsupported platforms,
        /// turn of the `unsupported` feature.
        pub const SUPPORTED_PLATFORM: bool = cfg!(any($($(target_os = $platform),*),*));
    }
}

// SINGLE deviation from the published xattr 0.2.3 tarball
// (sha256 6d1526bbe5aaeb5eb06885f4d987bcdfa5e23187055de9b83fe00156a821fabc):
// the `platforms!` macro emitted `libc::ENOATTR` for EVERY supported group,
// but libc no longer defines ENOATTR on linux/android (glibc spells the same
// errno errno 61 `ENODATA`; <sys/xattr.h> defines ENOATTR == ENODATA, and
// this crate's util.rs only *compares* against the value). cfg-split the two
// spellings; macOS/BSD keep libc::ENOATTR. Upstream is abandoned (0.2.3,
// 2020; crates.io 2026-11-05), pulled in via zed-async-tar -> gpui.
platforms! {
    "android"; "linux"; "macos" => linux_macos,
    "freebsd"; "netbsd" => bsd
}
