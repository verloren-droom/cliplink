use super::*;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn};

impl AppDelegate {
    pub(super) fn present_pending_trust_prompt_if_needed(&self) {
        let pending_request = self.ivars().controller.borrow().pending_trust_request();
        let Some(request) = pending_request else {
            self.ivars().active_trust_prompt_id.replace(None);
            return;
        };

        if self.ivars().active_trust_prompt_id.borrow().as_ref() == Some(&request.request_id) {
            return;
        }

        self.ivars()
            .active_trust_prompt_id
            .replace(Some(request.request_id));

        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str("新的设备连接请求"));
        alert.setInformativeText(&NSString::from_str(&format!(
            "设备“{}”希望建立受信任连接。\n\n地址: {}\n指纹: {}\n\n选择“拒绝”将立即终止本次连接请求。",
            request.device_name, request.secondary_text, request.fingerprint
        )));
        alert.addButtonWithTitle(&NSString::from_str("同意"));
        alert.addButtonWithTitle(&NSString::from_str("拒绝"));

        let allow = alert.runModal() == NSAlertFirstButtonReturn;
        let response = self.with_controller_mut(|controller| {
            controller.respond_to_trust_request(request.request_id, allow)
        });
        self.ivars().active_trust_prompt_id.replace(None);

        if let Err(error) = response {
            self.report_controller_status(format!("处理连接请求失败: {error}"));
        }

        if self.panel_visible() {
            self.refresh_panel_feedback_state();
        }
        if self
            .preferences_window()
            .is_some_and(|window| window.isVisible())
        {
            self.refresh_preferences_runtime_state();
        }
    }
}
