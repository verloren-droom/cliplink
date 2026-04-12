// Cargo requires the build script entry file to stay at the repository root.
// The actual asset generation logic lives under tools/build_support/.
#[cfg(target_os = "macos")]
#[path = "tools/build_support/icon_assets.rs"]
mod icon_assets;

fn main() {
    #[cfg(target_os = "macos")]
    icon_assets::generate();
}
