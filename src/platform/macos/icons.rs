use super::*;

use objc2::AnyThread;
use objc2_foundation::NSData;

const MACOS_TRAY_ICON_PNG: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/cliplink-macos-tray-icon-64.png"));
const MACOS_APP_ICON_PNG: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/cliplink-macos-app-icon-256.png"));

fn image_from_png_bytes(bytes: &'static [u8]) -> Option<Retained<NSImage>> {
    let data =
        unsafe { NSData::dataWithBytes_length(bytes.as_ptr().cast(), bytes.len() as NSUInteger) };
    NSImage::initWithData(NSImage::alloc(), &data)
}

impl AppDelegate {
    pub(super) fn application_icon_image(&self) -> &NSImage {
        self.ivars().application_icon.get_or_init(|| {
            image_from_png_bytes(MACOS_APP_ICON_PNG)
                .expect("failed to decode generated macOS application icon PNG")
        })
    }

    pub(super) fn status_item_icon_image(&self) -> &NSImage {
        self.ivars().status_item_icon.get_or_init(|| {
            let image = image_from_png_bytes(MACOS_TRAY_ICON_PNG)
                .expect("failed to decode generated macOS status item icon PNG");
            image.setTemplate(true);
            image.setSize(NSSize::new(18.0, 18.0));
            image
        })
    }
}
