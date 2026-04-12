use super::*;
use objc2_foundation::ns_string;

use crate::constants::app::APP_NAME;
use crate::platform::macos::widgets::{make_field_label, make_secondary_label};

const ABOUT_WIDTH: f64 = 368.0;
const ABOUT_HEIGHT: f64 = 228.0;
const ABOUT_ICON_SIZE: f64 = 72.0;

impl AppDelegate {
    pub(super) fn install_about_window(&self, mtm: MainThreadMarker) {
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(ABOUT_WIDTH, ABOUT_HEIGHT),
                ),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(ns_string!("关于"));
        window.center();
        window.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
        window.setDelegate(Some(ProtocolObject::from_ref(self)));

        let Some(content) = window.contentView() else {
            return;
        };

        let icon = NSImageView::imageViewWithImage(self.application_icon_image(), mtm);
        icon.setFrame(NSRect::new(
            NSPoint::new((ABOUT_WIDTH - ABOUT_ICON_SIZE) * 0.5, ABOUT_HEIGHT - 98.0),
            NSSize::new(ABOUT_ICON_SIZE, ABOUT_ICON_SIZE),
        ));
        icon.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        content.addSubview(&icon);

        let title = make_field_label(
            mtm,
            APP_NAME,
            NSRect::new(
                NSPoint::new(36.0, ABOUT_HEIGHT - 130.0),
                NSSize::new(ABOUT_WIDTH - 72.0, 28.0),
            ),
        );
        title.setAlignment(objc2_app_kit::NSTextAlignment::Center);
        title.setFont(Some(&NSFont::boldSystemFontOfSize(20.0)));

        let version = make_secondary_label(
            mtm,
            &format!("版本 {}", env!("CARGO_PKG_VERSION")),
            NSRect::new(
                NSPoint::new(36.0, ABOUT_HEIGHT - 158.0),
                NSSize::new(ABOUT_WIDTH - 72.0, 18.0),
            ),
        );
        version.setAlignment(objc2_app_kit::NSTextAlignment::Center);

        let description = make_secondary_label(
            mtm,
            "局域网剪贴板共享工具",
            NSRect::new(
                NSPoint::new(36.0, ABOUT_HEIGHT - 184.0),
                NSSize::new(ABOUT_WIDTH - 72.0, 18.0),
            ),
        );
        description.setAlignment(objc2_app_kit::NSTextAlignment::Center);

        content.addSubview(&title);
        content.addSubview(&version);
        content.addSubview(&description);

        self.ivars().about_window.replace(Some(window));
    }

    pub(super) fn ensure_about_window(&self) {
        if self.about_window().is_none() {
            self.install_about_window(self.mtm());
        }
    }

    pub(super) fn show_about_window(&self) {
        self.hide_panel(false);
        self.ensure_about_window();

        let mtm = self.mtm();
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);

        if let Some(window) = self.about_window() {
            window.makeKeyAndOrderFront(None);
        }
    }

    pub(super) fn hide_about_window(&self) {
        let window = self.ivars().about_window.borrow_mut().take();
        if let Some(window) = window {
            window.orderOut(None);
            window.setContentView(None);
            window.close();
        }
    }
}
