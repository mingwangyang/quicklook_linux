fn main() {
    for library in [
        "mpv",
        "epoxy",
        "egl",
        "libarchive",
        "vips",
        "libraw",
        "fontconfig",
        "x11",
    ] {
        pkg_config::probe_library(library).unwrap_or_else(|e| panic!("{library}: {e}"));
    }
}
