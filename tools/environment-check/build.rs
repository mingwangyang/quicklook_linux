fn main() {
    if std::env::var_os("CARGO_FEATURE_FULL").is_some() {
        for library in [
            "poppler-glib",
            "mpv",
            "libarchive",
            "libheif",
            "libraw",
            "libjxl",
            "vips",
        ] {
            pkg_config::Config::new()
                .probe(library)
                .unwrap_or_else(|error| panic!("Cannot link {library}: {error}"));
        }
    }
}
