/// User-visible release name; the Cargo package version remains valid SemVer.
pub const VERSION: &str = concat!("V", env!("CARGO_PKG_VERSION"));

pub mod compositor;
pub mod config;
pub mod files;
pub mod hosts;
pub mod keys;
pub mod niri;
pub mod protocol;
pub mod text;
