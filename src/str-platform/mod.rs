//! Platform-specific helpers, selected at compile time.

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub use windows as native;

#[cfg(all(unix, not(target_os = "macos")))]
pub mod linux;
#[cfg(all(unix, not(target_os = "macos")))]
pub use linux as native;

#[cfg(target_os = "macos")]
pub mod macos_stub;
#[cfg(target_os = "macos")]
pub use macos_stub as native;

/// Shared result type for platform helpers.
pub type PlatformResult<T> = Result<T, String>;
