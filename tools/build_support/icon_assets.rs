use std::{
    env, fs,
    path::{Path, PathBuf},
};

use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg::{Options, Tree},
};

const ICON_SOURCE_PATH: &str = "resources/icon.svg";
const MACOS_TRAY_ICON_FILE: &str = "cliplink-macos-tray-icon-64.png";
const MACOS_APP_ICON_FILE: &str = "cliplink-macos-app-icon-256.png";
const MACOS_BUNDLE_ICNS_FILE: &str = "cliplink-macos-bundle-icon.icns";

struct RasterIconAsset {
    file_name: &'static str,
    size: u32,
}

const RASTER_ICON_ASSETS: &[RasterIconAsset] = &[
    RasterIconAsset {
        file_name: MACOS_TRAY_ICON_FILE,
        size: 64,
    },
    RasterIconAsset {
        file_name: MACOS_APP_ICON_FILE,
        size: 256,
    },
];

pub(crate) fn generate() {
    println!("cargo:rerun-if-changed={ICON_SOURCE_PATH}");

    let svg_data = fs::read(ICON_SOURCE_PATH)
        .unwrap_or_else(|error| panic!("failed to read {ICON_SOURCE_PATH}: {error}"));
    let tree = Tree::from_data(&svg_data, &Options::default())
        .unwrap_or_else(|error| panic!("failed to parse {ICON_SOURCE_PATH}: {error}"));
    let output_dir = PathBuf::from(
        env::var_os("OUT_DIR")
            .unwrap_or_else(|| panic!("missing OUT_DIR for generated icon assets")),
    );

    for asset in RASTER_ICON_ASSETS {
        write_png_file(
            &output_dir.join(asset.file_name),
            &render_png_bytes(&tree, asset.size),
        );
    }

    write_macos_icns(
        &output_dir.join(MACOS_BUNDLE_ICNS_FILE),
        &render_png_bytes(&tree, 1024),
    );
}

fn render_png_bytes(tree: &Tree, output_size: u32) -> Vec<u8> {
    let tree_size = tree.size();
    let scale_x = output_size as f32 / tree_size.width();
    let scale_y = output_size as f32 / tree_size.height();
    let mut pixmap = Pixmap::new(output_size, output_size)
        .unwrap_or_else(|| panic!("failed to allocate {output_size}x{output_size} icon pixmap"));

    resvg::render(
        &tree,
        Transform::from_scale(scale_x, scale_y),
        &mut pixmap.as_mut(),
    );

    pixmap.encode_png().unwrap_or_else(|error| {
        panic!("failed to encode generated {output_size}px icon PNG: {error}")
    })
}

fn write_png_file(output_path: &Path, png_data: &[u8]) {
    fs::write(output_path, png_data).unwrap_or_else(|error| {
        panic!(
            "failed to write generated icon {}: {error}",
            output_path.display()
        )
    });
}

fn write_macos_icns(output_path: &Path, png_data: &[u8]) {
    let icon_chunk_len = 8u32
        .checked_add(png_data.len() as u32)
        .unwrap_or_else(|| panic!("generated icon is too large for an icns chunk"));
    let total_len = 8u32
        .checked_add(icon_chunk_len)
        .unwrap_or_else(|| panic!("generated icon is too large for an icns file"));
    let mut icns_data = Vec::with_capacity(total_len as usize);

    icns_data.extend_from_slice(b"icns");
    icns_data.extend_from_slice(&total_len.to_be_bytes());
    icns_data.extend_from_slice(b"ic10");
    icns_data.extend_from_slice(&icon_chunk_len.to_be_bytes());
    icns_data.extend_from_slice(png_data);

    fs::write(output_path, icns_data).unwrap_or_else(|error| {
        panic!(
            "failed to write generated icon {}: {error}",
            output_path.display()
        )
    });
}
