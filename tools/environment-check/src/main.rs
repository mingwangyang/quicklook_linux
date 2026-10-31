use gtk4::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    gtk4::init()?;
    let display = gtk4::gdk::Display::default().ok_or("No display available")?;
    println!(
        "GTK {}.{}.{}; display type: {}",
        gtk4::major_version(),
        gtk4::minor_version(),
        gtk4::micro_version(),
        display.type_().name()
    );
    if !display.type_().name().contains("Wayland") {
        return Err("Expected a native Wayland display".into());
    }
    let bus =
        gtk4::gio::bus_get_sync(gtk4::gio::BusType::Session, None::<&gtk4::gio::Cancellable>)?;
    println!(
        "Session DBus connected: {}",
        bus.unique_name().ok_or("No session DBus unique name")?
    );
    println!("Rust / GTK4 / native Wayland / session DBus: OK");

    #[cfg(feature = "full")]
    check_backends()?;

    Ok(())
}

#[cfg(feature = "full")]
fn check_backends() -> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::{CStr, c_char, c_ulong};

    unsafe extern "C" {
        fn mpv_client_api_version() -> c_ulong;
        fn poppler_get_version() -> *const c_char;
        fn archive_version_string() -> *const c_char;
        fn heif_get_version() -> *const c_char;
        fn libraw_version() -> *const c_char;
        fn JxlDecoderVersion() -> u32;
        fn vips_version_string() -> *const c_char;
    }

    let languages = sourceview5::LanguageManager::default();
    if languages.language("rust").is_none() {
        return Err("GtkSourceView Rust language definition is missing".into());
    }
    println!("GtkSourceView language definitions: OK (Rust)");
    println!(
        "WebKit registered type: {}",
        webkit6::WebView::static_type()
    );

    // These C APIs return library-owned, NUL-terminated static version strings.
    unsafe {
        for (name, pointer) in [
            ("Poppler", poppler_get_version()),
            ("libarchive", archive_version_string()),
            ("libheif", heif_get_version()),
            ("LibRaw", libraw_version()),
            ("libvips", vips_version_string()),
        ] {
            if pointer.is_null() {
                return Err(format!("{name} returned a null version string").into());
            }
            println!("{name}: {}", CStr::from_ptr(pointer).to_string_lossy());
        }
        println!("libmpv client API: {:#x}", mpv_client_api_version());
        println!("JPEG XL decoder version: {}", JxlDecoderVersion());
    }
    println!("P0/P1 native library linking and version calls: OK");
    Ok(())
}
