mod clipboard;
mod crypto;
mod hotkey;
mod paste;

use std::{env, mem::size_of, path::PathBuf, ptr::null_mut};

use agnostic_mdns::hostname;
use directories::ProjectDirs;
use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject, GetSysColorBrush, HGDIOBJ},
    System::{
        DataExchange::{AddClipboardFormatListener, RemoveClipboardFormatListener},
        LibraryLoader::GetModuleHandleW,
    },
    UI::{
        Controls::{
            HKM_GETHOTKEY, HKM_SETHOTKEY, ICC_STANDARD_CLASSES, ICC_TAB_CLASSES, ICC_WIN95_CLASSES,
            INITCOMMONCONTROLSEX, InitCommonControlsEx, NMHDR, TCIF_TEXT, TCITEMW, TCM_GETCURSEL,
            TCM_INSERTITEMW, TCN_SELCHANGE,
        },
        Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey},
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            AppendMenuW, BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_PUSHBUTTON,
            BST_CHECKED, BST_UNCHECKED, COLOR_WINDOW, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW,
            CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
            DestroyWindow, DispatchMessageW, EM_SETLIMITTEXT, EN_CHANGE, EnableWindow,
            GWLP_USERDATA, GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW,
            GetWindowTextLengthW, GetWindowTextW, HCURSOR, HICON, HMENU, IDC_ARROW,
            IDI_APPLICATION, IsWindowVisible, LB_ADDSTRING, LB_GETCURSEL, LB_GETTOPINDEX,
            LB_RESETCONTENT, LB_SETCURSEL, LB_SETTOPINDEX, LBN_DBLCLK, LBN_SELCHANGE,
            LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, LoadCursorW, LoadIconW, MB_ICONINFORMATION, MB_OK,
            MF_SEPARATOR, MF_STRING, MSG, MessageBoxW, MoveWindow, PostQuitMessage, RegisterClassW,
            SPI_GETWORKAREA, SW_HIDE, SW_SHOW, SW_SHOWNORMAL, SendMessageW, SetFocus,
            SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowTextW, ShowWindow,
            SystemParametersInfoW, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
            TranslateMessage, UpdateWindow, WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_CLIPBOARDUPDATE,
            WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_HOTKEY, WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY,
            WM_SETFONT, WM_SIZE, WM_TIMER, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD,
            WS_EX_CLIENTEDGE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPED, WS_POPUP, WS_SYSMENU,
            WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
};

use crate::{
    constants::app::{
        APP_NAME, ORGANIZATION_NAME, ORGANIZATION_QUALIFIER, PRODUCT_DIR_NAME, STORAGE_DIR_NAME,
    },
    controller::{
        AppController, HistoryRow, SettingsDeviceEntry, SettingsSnapshot, SettingsUpdate,
    },
    core::{
        at_rest::LocalDataCipher,
        clipboard::ClipboardBackend,
        error::{AppError, AppResult},
        paths::AppPaths,
    },
    platform::PlatformResult,
};

use self::{
    clipboard::{
        create_clipboard_backend as create_windows_clipboard_backend, shared_clipboard_state,
    },
    crypto::create_local_data_cipher as create_windows_local_data_cipher,
    hotkey::{
        RegisteredHotKey, format_hotkey_for_display, hotkey_control_value,
        hotkey_from_control_value, parse_registered_hotkey,
    },
    paste::{capture_foreground_window, trigger_immediate_paste},
};

const MAIN_CLASS_NAME: &str = "ClipLinkWindowsMain";
const HISTORY_CLASS_NAME: &str = "ClipLinkWindowsHistory";
const PREFERENCES_CLASS_NAME: &str = "ClipLinkWindowsPreferences";
const HOTKEY_CONTROL_CLASS_NAME: &str = "msctls_hotkey32";

const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 1;
const APP_TIMER_ID: usize = 1;
const GLOBAL_HOTKEY_ID: i32 = 1;

const WINDOW_WIDTH_HISTORY: i32 = 440;
const WINDOW_HEIGHT_HISTORY: i32 = 416;
const WINDOW_WIDTH_PREFERENCES: i32 = 560;
const WINDOW_HEIGHT_PREFERENCES: i32 = 500;
const UI_TICK_INTERVAL_MS: u32 = 250;

const ID_HISTORY_SEARCH: i32 = 1001;
const ID_HISTORY_LIST: i32 = 1002;
const ID_HISTORY_DETAIL: i32 = 1003;
const ID_HISTORY_HOTKEY: i32 = 1004;
const ID_HISTORY_CLEAR: i32 = 1005;
const ID_HISTORY_DELETE: i32 = 1006;
const ID_HISTORY_PREFERENCES: i32 = 1007;
const ID_HISTORY_QUIT: i32 = 1008;

const ID_PREFS_TAB: i32 = 2001;
const ID_PREFS_DEVICE_NAME: i32 = 2002;
const ID_PREFS_HISTORY_LIMIT: i32 = 2003;
const ID_PREFS_SHARE_LOCAL: i32 = 2004;
const ID_PREFS_PREFER_REMOTE: i32 = 2005;
const ID_PREFS_DISCOVERY: i32 = 2006;
const ID_PREFS_DEVICES_LIST: i32 = 2007;
const ID_PREFS_TRUST: i32 = 2008;
const ID_PREFS_REVOKE: i32 = 2009;
const ID_PREFS_HOTKEY: i32 = 2010;
const ID_PREFS_STATUS: i32 = 2011;
const ID_PREFS_SAVE: i32 = 2012;
const ID_PREFS_CLOSE: i32 = 2013;

const ID_MENU_SHOW_HISTORY: u32 = 3001;
const ID_MENU_OPEN_PREFERENCES: u32 = 3002;
const ID_MENU_OPEN_ABOUT: u32 = 3003;
const ID_MENU_QUIT: u32 = 3004;

const TAB_GENERAL: i32 = 0;
const TAB_SHARING: i32 = 1;
const TAB_HOTKEY: i32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreferencesRawState {
    device_name: String,
    history_limit_text: String,
    hotkey: String,
    share_local_history: bool,
    prefer_remote_latest_on_paste: bool,
    discovery_enabled: bool,
}

#[derive(Default)]
struct HistoryControls {
    hwnd: HWND,
    search: HWND,
    list: HWND,
    detail: HWND,
    hotkey: HWND,
    clear_button: HWND,
    delete_button: HWND,
    preferences_button: HWND,
    quit_button: HWND,
}

#[derive(Default)]
struct PreferencesControls {
    hwnd: HWND,
    tab: HWND,
    device_name_label: HWND,
    device_name_input: HWND,
    history_limit_label: HWND,
    history_limit_input: HWND,
    share_checkbox: HWND,
    prefer_remote_checkbox: HWND,
    discovery_checkbox: HWND,
    devices_label: HWND,
    devices_list: HWND,
    trust_button: HWND,
    revoke_button: HWND,
    hotkey_label: HWND,
    hotkey_input: HWND,
    status_label: HWND,
    save_button: HWND,
    close_button: HWND,
}

struct WindowsApp {
    instance: HINSTANCE,
    default_font: isize,
    controller: AppController,
    main_hwnd: HWND,
    history: HistoryControls,
    preferences: PreferencesControls,
    registered_hotkey: Option<RegisteredHotKey>,
    history_rows: Vec<HistoryRow>,
    preferences_devices: Vec<SettingsDeviceEntry>,
    preferences_baseline_raw: Option<PreferencesRawState>,
    previous_foreground: Option<HWND>,
}

pub(super) fn device_name_hint() -> Option<String> {
    hostname()
        .map(|value| value.to_string())
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| env::var("COMPUTERNAME").ok())
}

pub(super) fn discover_app_paths() -> AppResult<AppPaths> {
    let root = ProjectDirs::from(ORGANIZATION_QUALIFIER, ORGANIZATION_NAME, PRODUCT_DIR_NAME)
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| {
            env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(format!(".{STORAGE_DIR_NAME}-data"))
        });

    let paths = AppPaths::from_root(root);
    paths.ensure()?;
    Ok(paths)
}

pub(super) fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    create_windows_clipboard_backend()
}

pub(super) fn create_local_data_cipher(paths: &AppPaths) -> AppResult<LocalDataCipher> {
    create_windows_local_data_cipher(paths)
}

pub(super) fn run(controller: AppController) -> PlatformResult {
    unsafe {
        let mut app = Box::new(WindowsApp::new(controller)?);
        app.initialize()?;
        app.message_loop();
        app.shutdown();
    }
    Ok(())
}

impl WindowsApp {
    fn new(controller: AppController) -> AppResult<Self> {
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        if instance == 0 {
            return Err(AppError::InvalidConfig(
                "Failed to obtain the Windows module handle.".to_string(),
            ));
        }

        Ok(Self {
            instance,
            default_font: unsafe { GetStockObject(DEFAULT_GUI_FONT) as HGDIOBJ as isize },
            controller,
            main_hwnd: 0,
            history: HistoryControls::default(),
            preferences: PreferencesControls::default(),
            registered_hotkey: None,
            history_rows: Vec::new(),
            preferences_devices: Vec::new(),
            preferences_baseline_raw: None,
            previous_foreground: None,
        })
    }

    unsafe fn initialize(&mut self) -> AppResult<()> {
        self.initialize_common_controls();
        self.register_window_class(MAIN_CLASS_NAME)?;
        self.register_window_class(HISTORY_CLASS_NAME)?;
        self.register_window_class(PREFERENCES_CLASS_NAME)?;
        self.main_hwnd =
            self.create_top_level_window(MAIN_CLASS_NAME, APP_NAME, WS_OVERLAPPED, 0, 0, 0, 0)?;
        self.install_clipboard_listener()?;
        self.install_tray_icon()?;
        self.refresh_global_hotkey()?;
        SetTimer(self.main_hwnd, APP_TIMER_ID, UI_TICK_INTERVAL_MS, None);
        Ok(())
    }

    unsafe fn shutdown(&mut self) {
        if self.main_hwnd != 0 {
            let _ = RemoveClipboardFormatListener(self.main_hwnd);
        }

        if self.registered_hotkey.take().is_some() && self.main_hwnd != 0 {
            let _ = UnregisterHotKey(self.main_hwnd, GLOBAL_HOTKEY_ID);
        }

        if self.main_hwnd != 0 {
            let mut tray = self.tray_data();
            let _ = Shell_NotifyIconW(NIM_DELETE, &mut tray);
        }

        if self.history.hwnd != 0 {
            let _ = DestroyWindow(self.history.hwnd);
        }
        if self.preferences.hwnd != 0 {
            let _ = DestroyWindow(self.preferences.hwnd);
        }
        if self.main_hwnd != 0 {
            let _ = DestroyWindow(self.main_hwnd);
        }
    }

    unsafe fn initialize_common_controls(&self) {
        let mut controls = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES | ICC_TAB_CLASSES | ICC_WIN95_CLASSES,
        };
        InitCommonControlsEx(&mut controls);
    }

    unsafe fn register_window_class(&self, class_name: &str) -> AppResult<()> {
        let class_name_wide = wide(class_name);
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(app_wndproc),
            hInstance: self.instance,
            lpszClassName: class_name_wide.as_ptr(),
            hCursor: LoadCursorW(0, IDC_ARROW as _),
            hIcon: LoadIconW(0, IDI_APPLICATION as _),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW as i32),
            ..std::mem::zeroed()
        };

        if RegisterClassW(&wc) == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(1410) {
                return Err(AppError::InvalidConfig(format!(
                    "Failed to register Windows class `{class_name}`: {error}"
                )));
            }
        }
        Ok(())
    }

    unsafe fn create_top_level_window(
        &mut self,
        class_name: &str,
        title: &str,
        style: u32,
        ex_style: u32,
        width: i32,
        height: i32,
    ) -> AppResult<HWND> {
        let class_name = wide(class_name);
        let title = wide(title);
        let hwnd = CreateWindowExW(
            ex_style,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            0,
            0,
            self.instance,
            self as *mut _ as _,
        );

        if hwnd == 0 {
            return Err(AppError::InvalidConfig(format!(
                "Failed to create a Windows top-level window: {}",
                std::io::Error::last_os_error()
            )));
        }

        Ok(hwnd)
    }

    unsafe fn install_clipboard_listener(&self) -> AppResult<()> {
        if AddClipboardFormatListener(self.main_hwnd) == 0 {
            return Err(AppError::Clipboard(format!(
                "Failed to subscribe to Windows clipboard updates: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    unsafe fn install_tray_icon(&self) -> AppResult<()> {
        let mut tray = self.tray_data();
        if Shell_NotifyIconW(NIM_ADD, &mut tray) == 0 {
            return Err(AppError::InvalidConfig(format!(
                "Failed to create the Windows tray icon: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    unsafe fn tray_data(&self) -> NOTIFYICONDATAW {
        let mut tray: NOTIFYICONDATAW = std::mem::zeroed();
        tray.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        tray.hWnd = self.main_hwnd;
        tray.uID = 1;
        tray.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        tray.uCallbackMessage = TRAY_CALLBACK_MESSAGE;
        tray.hIcon = LoadIconW(0, IDI_APPLICATION as _);
        copy_wide_into_fixed(&wide(APP_NAME), &mut tray.szTip);
        tray
    }

    unsafe fn message_loop(&mut self) {
        let mut message = MSG::default();
        while GetMessageW(&mut message, 0, 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    unsafe fn window_proc(
        &mut self,
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if hwnd == self.main_hwnd {
            return self.main_window_proc(hwnd, message, wparam, lparam);
        }
        if hwnd == self.history.hwnd {
            return self.history_window_proc(hwnd, message, wparam, lparam);
        }
        if hwnd == self.preferences.hwnd {
            return self.preferences_window_proc(hwnd, message, wparam, lparam);
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }

    unsafe fn main_window_proc(
        &mut self,
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_TIMER => {
                self.tick_controller();
                0
            }
            WM_CLIPBOARDUPDATE => {
                shared_clipboard_state().notify_changed();
                0
            }
            WM_HOTKEY => {
                self.toggle_history_popup();
                0
            }
            TRAY_CALLBACK_MESSAGE => {
                self.handle_tray_callback(lparam as u32);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe fn history_window_proc(
        &mut self,
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_COMMAND => {
                let control_id = loword(wparam as usize) as i32;
                let notification = hiword(wparam as usize);
                self.handle_history_command(control_id, notification);
                0
            }
            WM_ACTIVATE => {
                if loword(wparam as usize) == WA_INACTIVE as u16 {
                    self.hide_history_popup();
                }
                0
            }
            WM_CLOSE => {
                ShowWindow(hwnd, SW_HIDE);
                0
            }
            WM_SIZE => {
                self.layout_history_controls();
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe fn preferences_window_proc(
        &mut self,
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_COMMAND => {
                let control_id = loword(wparam as usize) as i32;
                let notification = hiword(wparam as usize);
                self.handle_preferences_command(control_id, notification);
                0
            }
            WM_NOTIFY => {
                let header = &*(lparam as *const NMHDR);
                if header.idFrom as i32 == ID_PREFS_TAB && header.code == TCN_SELCHANGE as u32 {
                    self.update_preferences_tab_visibility();
                    return 0;
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_CLOSE => {
                self.hide_preferences();
                0
            }
            WM_SIZE => {
                self.layout_preferences_controls();
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe fn tick_controller(&mut self) {
        let outcome = self.controller.tick();
        if outcome.history_changed
            && self.history.hwnd != 0
            && IsWindowVisible(self.history.hwnd) != 0
        {
            self.refresh_history_list(true);
        }
        if self.history.hwnd != 0
            && IsWindowVisible(self.history.hwnd) != 0
            && outcome.status_changed
        {
            self.update_history_detail_label();
        }
        if self.preferences.hwnd != 0
            && IsWindowVisible(self.preferences.hwnd) != 0
            && (outcome.devices_changed || outcome.status_changed)
        {
            self.refresh_preferences_devices_and_status();
        }
    }

    unsafe fn handle_tray_callback(&mut self, event: u32) {
        match event {
            value if value == windows_sys::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP => {
                self.toggle_history_popup();
            }
            value if value == windows_sys::Win32::UI::WindowsAndMessaging::WM_RBUTTONUP => {
                self.show_tray_menu();
            }
            _ => {}
        }
    }

    unsafe fn show_tray_menu(&mut self) {
        let menu = CreatePopupMenu();
        if menu == 0 {
            return;
        }

        let history = wide("历史记录");
        let preferences = wide("偏好设置...");
        let about = wide("关于");
        let quit = wide("退出");
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_MENU_SHOW_HISTORY as usize,
            history.as_ptr(),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_MENU_OPEN_PREFERENCES as usize,
            preferences.as_ptr(),
        );
        let _ = AppendMenuW(menu, MF_STRING, ID_MENU_OPEN_ABOUT as usize, about.as_ptr());
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_MENU_QUIT as usize, quit.as_ptr());

        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(self.main_hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            point.x,
            point.y,
            0,
            self.main_hwnd,
            std::ptr::null(),
        ) as u32;
        let _ = DestroyMenu(menu);

        match command {
            ID_MENU_SHOW_HISTORY => self.show_history_popup(),
            ID_MENU_OPEN_PREFERENCES => self.open_preferences(),
            ID_MENU_OPEN_ABOUT => self.show_about(),
            ID_MENU_QUIT => self.quit(),
            _ => {}
        }
    }

    unsafe fn ensure_history_window(&mut self) -> AppResult<()> {
        if self.history.hwnd != 0 {
            return Ok(());
        }

        let hwnd = self.create_top_level_window(
            HISTORY_CLASS_NAME,
            APP_NAME,
            WS_POPUP | WS_BORDER,
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            WINDOW_WIDTH_HISTORY,
            WINDOW_HEIGHT_HISTORY,
        )?;
        self.history.hwnd = hwnd;
        self.history.search = self.create_child_window(
            hwnd,
            "Edit",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
            WS_EX_CLIENTEDGE,
            ID_HISTORY_SEARCH,
        )?;
        self.history.list = self.create_child_window(
            hwnd,
            "ListBox",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT,
            WS_EX_CLIENTEDGE,
            ID_HISTORY_LIST,
        )?;
        self.history.detail = self.create_child_window(
            hwnd,
            "Static",
            "",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_HISTORY_DETAIL,
        )?;
        self.history.hotkey = self.create_child_window(
            hwnd,
            "Static",
            "",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_HISTORY_HOTKEY,
        )?;
        self.history.clear_button = self.create_child_window(
            hwnd,
            "Button",
            "清除",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            0,
            ID_HISTORY_CLEAR,
        )?;
        self.history.delete_button = self.create_child_window(
            hwnd,
            "Button",
            "删除",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            0,
            ID_HISTORY_DELETE,
        )?;
        self.history.preferences_button = self.create_child_window(
            hwnd,
            "Button",
            "偏好设置",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            0,
            ID_HISTORY_PREFERENCES,
        )?;
        self.history.quit_button = self.create_child_window(
            hwnd,
            "Button",
            "退出",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            0,
            ID_HISTORY_QUIT,
        )?;

        self.apply_default_font(self.history.search);
        self.apply_default_font(self.history.list);
        self.apply_default_font(self.history.detail);
        self.apply_default_font(self.history.hotkey);
        self.apply_default_font(self.history.clear_button);
        self.apply_default_font(self.history.delete_button);
        self.apply_default_font(self.history.preferences_button);
        self.apply_default_font(self.history.quit_button);
        let _ = SendMessageW(self.history.search, EM_SETLIMITTEXT, 256, 0);
        self.layout_history_controls();
        Ok(())
    }

    unsafe fn ensure_preferences_window(&mut self) -> AppResult<()> {
        if self.preferences.hwnd != 0 {
            return Ok(());
        }

        let hwnd = self.create_top_level_window(
            PREFERENCES_CLASS_NAME,
            "偏好设置",
            WS_CAPTION | WS_SYSMENU | WS_OVERLAPPED,
            WS_EX_TOOLWINDOW,
            WINDOW_WIDTH_PREFERENCES,
            WINDOW_HEIGHT_PREFERENCES,
        )?;
        self.preferences.hwnd = hwnd;
        self.preferences.tab = self.create_child_window(
            hwnd,
            "SysTabControl32",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_PREFS_TAB,
        )?;
        self.preferences.device_name_label =
            self.create_child_window(hwnd, "Static", "设备名称", WS_CHILD | WS_VISIBLE, 0, 0)?;
        self.preferences.device_name_input = self.create_child_window(
            hwnd,
            "Edit",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
            WS_EX_CLIENTEDGE,
            ID_PREFS_DEVICE_NAME,
        )?;
        self.preferences.history_limit_label =
            self.create_child_window(hwnd, "Static", "历史记录数量", WS_CHILD | WS_VISIBLE, 0, 0)?;
        self.preferences.history_limit_input = self.create_child_window(
            hwnd,
            "Edit",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
            WS_EX_CLIENTEDGE,
            ID_PREFS_HISTORY_LIMIT,
        )?;
        self.preferences.share_checkbox = self.create_child_window(
            hwnd,
            "Button",
            "共享本机剪切板历史",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX,
            0,
            ID_PREFS_SHARE_LOCAL,
        )?;
        self.preferences.prefer_remote_checkbox = self.create_child_window(
            hwnd,
            "Button",
            "粘贴时优先远端最新内容",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX,
            0,
            ID_PREFS_PREFER_REMOTE,
        )?;
        self.preferences.discovery_checkbox = self.create_child_window(
            hwnd,
            "Button",
            "偏好设置打开时发现可用设备",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX,
            0,
            ID_PREFS_DISCOVERY,
        )?;
        self.preferences.devices_label =
            self.create_child_window(hwnd, "Static", "设备列表", WS_CHILD | WS_VISIBLE, 0, 0)?;
        self.preferences.devices_list = self.create_child_window(
            hwnd,
            "ListBox",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT,
            WS_EX_CLIENTEDGE,
            ID_PREFS_DEVICES_LIST,
        )?;
        self.preferences.trust_button = self.create_child_window(
            hwnd,
            "Button",
            "设为信任",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            0,
            ID_PREFS_TRUST,
        )?;
        self.preferences.revoke_button = self.create_child_window(
            hwnd,
            "Button",
            "移除信任",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            0,
            ID_PREFS_REVOKE,
        )?;
        self.preferences.hotkey_label = self.create_child_window(
            hwnd,
            "Static",
            "显示历史列表快捷键",
            WS_CHILD | WS_VISIBLE,
            0,
            0,
        )?;
        self.preferences.hotkey_input = self.create_child_window(
            hwnd,
            HOTKEY_CONTROL_CLASS_NAME,
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
            WS_EX_CLIENTEDGE,
            ID_PREFS_HOTKEY,
        )?;
        self.preferences.status_label = self.create_child_window(
            hwnd,
            "Static",
            "",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_PREFS_STATUS,
        )?;
        self.preferences.save_button = self.create_child_window(
            hwnd,
            "Button",
            "保存",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            0,
            ID_PREFS_SAVE,
        )?;
        self.preferences.close_button = self.create_child_window(
            hwnd,
            "Button",
            "关闭",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            0,
            ID_PREFS_CLOSE,
        )?;

        for hwnd in [
            self.preferences.tab,
            self.preferences.device_name_label,
            self.preferences.device_name_input,
            self.preferences.history_limit_label,
            self.preferences.history_limit_input,
            self.preferences.share_checkbox,
            self.preferences.prefer_remote_checkbox,
            self.preferences.discovery_checkbox,
            self.preferences.devices_label,
            self.preferences.devices_list,
            self.preferences.trust_button,
            self.preferences.revoke_button,
            self.preferences.hotkey_label,
            self.preferences.hotkey_input,
            self.preferences.status_label,
            self.preferences.save_button,
            self.preferences.close_button,
        ] {
            self.apply_default_font(hwnd);
        }

        self.insert_preferences_tab(TAB_GENERAL, "常规");
        self.insert_preferences_tab(TAB_SHARING, "共享");
        self.insert_preferences_tab(TAB_HOTKEY, "快捷键");
        let _ = SendMessageW(self.preferences.device_name_input, EM_SETLIMITTEXT, 64, 0);
        let _ = SendMessageW(self.preferences.history_limit_input, EM_SETLIMITTEXT, 4, 0);
        self.layout_preferences_controls();
        self.update_preferences_tab_visibility();
        Ok(())
    }

    unsafe fn create_child_window(
        &self,
        parent: HWND,
        class_name: &str,
        title: &str,
        style: u32,
        ex_style: u32,
        id: i32,
    ) -> AppResult<HWND> {
        let class_name = wide(class_name);
        let title = wide(title);
        let hwnd = CreateWindowExW(
            ex_style,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            0,
            0,
            0,
            0,
            parent,
            id as HMENU,
            self.instance,
            null_mut(),
        );
        if hwnd == 0 {
            return Err(AppError::InvalidConfig(format!(
                "Failed to create a Windows child control: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(hwnd)
    }

    unsafe fn insert_preferences_tab(&self, index: i32, title: &str) {
        let mut title = wide(title);
        let mut item = TCITEMW {
            mask: TCIF_TEXT,
            pszText: title.as_mut_ptr(),
            ..std::mem::zeroed()
        };
        let _ = SendMessageW(
            self.preferences.tab,
            TCM_INSERTITEMW,
            index as usize,
            &mut item as *mut _ as isize,
        );
    }

    unsafe fn apply_default_font(&self, hwnd: HWND) {
        let _ = SendMessageW(hwnd, WM_SETFONT, self.default_font as usize, 1);
    }

    unsafe fn layout_history_controls(&self) {
        if self.history.hwnd == 0 {
            return;
        }
        let mut rect = RECT::default();
        let _ = GetClientRect(self.history.hwnd, &mut rect);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let padding = 12;
        let button_width = 82;
        let button_height = 28;

        let _ = MoveWindow(
            self.history.search,
            padding,
            padding,
            width - padding * 2,
            28,
            1,
        );
        let _ = MoveWindow(
            self.history.list,
            padding,
            48,
            width - padding * 2,
            height - 160,
            1,
        );
        let _ = MoveWindow(
            self.history.detail,
            padding,
            height - 104,
            width - padding * 2,
            32,
            1,
        );
        let _ = MoveWindow(self.history.hotkey, padding, height - 72, 160, 20, 1);
        let _ = MoveWindow(
            self.history.clear_button,
            width - padding - button_width * 4 - 18,
            height - 76,
            button_width,
            button_height,
            1,
        );
        let _ = MoveWindow(
            self.history.delete_button,
            width - padding - button_width * 3 - 12,
            height - 76,
            button_width,
            button_height,
            1,
        );
        let _ = MoveWindow(
            self.history.preferences_button,
            width - padding - button_width * 2 - 6,
            height - 76,
            button_width,
            button_height,
            1,
        );
        let _ = MoveWindow(
            self.history.quit_button,
            width - padding - button_width,
            height - 76,
            button_width,
            button_height,
            1,
        );
    }

    unsafe fn layout_preferences_controls(&self) {
        if self.preferences.hwnd == 0 {
            return;
        }

        let mut rect = RECT::default();
        let _ = GetClientRect(self.preferences.hwnd, &mut rect);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let padding = 16;
        let footer_top = height - 54;
        let page_left = 26;
        let page_top = 52;
        let page_width = width - 52;

        let _ = MoveWindow(
            self.preferences.tab,
            padding,
            12,
            width - padding * 2,
            380,
            1,
        );
        let _ = MoveWindow(
            self.preferences.device_name_label,
            page_left,
            page_top + 10,
            90,
            20,
            1,
        );
        let _ = MoveWindow(
            self.preferences.device_name_input,
            page_left,
            page_top + 34,
            page_width,
            28,
            1,
        );
        let _ = MoveWindow(
            self.preferences.history_limit_label,
            page_left,
            page_top + 80,
            120,
            20,
            1,
        );
        let _ = MoveWindow(
            self.preferences.history_limit_input,
            page_left,
            page_top + 104,
            120,
            28,
            1,
        );

        let _ = MoveWindow(
            self.preferences.share_checkbox,
            page_left,
            page_top + 10,
            page_width,
            24,
            1,
        );
        let _ = MoveWindow(
            self.preferences.prefer_remote_checkbox,
            page_left,
            page_top + 42,
            page_width,
            24,
            1,
        );
        let _ = MoveWindow(
            self.preferences.discovery_checkbox,
            page_left,
            page_top + 74,
            page_width,
            24,
            1,
        );
        let _ = MoveWindow(
            self.preferences.devices_label,
            page_left,
            page_top + 114,
            100,
            20,
            1,
        );
        let _ = MoveWindow(
            self.preferences.devices_list,
            page_left,
            page_top + 138,
            page_width,
            150,
            1,
        );
        let _ = MoveWindow(
            self.preferences.trust_button,
            page_left,
            page_top + 300,
            90,
            28,
            1,
        );
        let _ = MoveWindow(
            self.preferences.revoke_button,
            page_left + 102,
            page_top + 300,
            90,
            28,
            1,
        );

        let _ = MoveWindow(
            self.preferences.hotkey_label,
            page_left,
            page_top + 10,
            180,
            20,
            1,
        );
        let _ = MoveWindow(
            self.preferences.hotkey_input,
            page_left,
            page_top + 36,
            220,
            28,
            1,
        );

        let _ = MoveWindow(
            self.preferences.status_label,
            padding,
            footer_top,
            width - 220,
            20,
            1,
        );
        let _ = MoveWindow(
            self.preferences.save_button,
            width - 184,
            footer_top - 6,
            76,
            30,
            1,
        );
        let _ = MoveWindow(
            self.preferences.close_button,
            width - 96,
            footer_top - 6,
            76,
            30,
            1,
        );
    }

    unsafe fn toggle_history_popup(&mut self) {
        if self.history.hwnd != 0 && IsWindowVisible(self.history.hwnd) != 0 {
            self.hide_history_popup();
        } else {
            self.show_history_popup();
        }
    }

    unsafe fn show_history_popup(&mut self) {
        if self.ensure_history_window().is_err() {
            return;
        }

        self.previous_foreground = capture_foreground_window(
            &[self.main_hwnd, self.history.hwnd, self.preferences.hwnd]
                .into_iter()
                .filter(|hwnd| *hwnd != 0)
                .collect::<Vec<_>>()
                .as_slice(),
        );
        self.set_edit_text(self.history.search, "");
        self.refresh_history_list(false);
        self.position_history_window();
        ShowWindow(self.history.hwnd, SW_SHOW);
        SetForegroundWindow(self.history.hwnd);
        SetFocus(self.history.list);
    }

    unsafe fn hide_history_popup(&self) {
        if self.history.hwnd != 0 {
            ShowWindow(self.history.hwnd, SW_HIDE);
        }
    }

    unsafe fn position_history_window(&self) {
        let mut cursor = POINT::default();
        let mut work_area = RECT::default();
        let _ = GetCursorPos(&mut cursor);
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work_area as *mut _ as *mut _, 0);

        let x = (cursor.x - WINDOW_WIDTH_HISTORY / 2).clamp(
            work_area.left + 8,
            work_area.right - WINDOW_WIDTH_HISTORY - 8,
        );
        let mut y = cursor.y - WINDOW_HEIGHT_HISTORY - 24;
        if y < work_area.top + 8 {
            y = (cursor.y + 20).clamp(
                work_area.top + 8,
                work_area.bottom - WINDOW_HEIGHT_HISTORY - 8,
            );
        }
        let _ = MoveWindow(
            self.history.hwnd,
            x,
            y,
            WINDOW_WIDTH_HISTORY,
            WINDOW_HEIGHT_HISTORY,
            1,
        );
    }

    unsafe fn refresh_history_list(&mut self, preserve_scroll: bool) {
        if self.history.list == 0 {
            return;
        }

        let selected_id = self.selected_history_id();
        let top_index = if preserve_scroll {
            SendMessageW(self.history.list, LB_GETTOPINDEX, 0, 0) as i32
        } else {
            0
        };
        let query = self.read_window_text(self.history.search);
        self.history_rows = self.controller.history_rows(&query);

        let _ = SendMessageW(self.history.list, LB_RESETCONTENT, 0, 0);
        for row in &self.history_rows {
            let line = wide(&format!("{}  {}", row.source_badge, row.summary_text));
            let _ = SendMessageW(self.history.list, LB_ADDSTRING, 0, line.as_ptr() as isize);
        }

        if let Some(id) = selected_id {
            if let Some(index) = self.history_rows.iter().position(|row| row.id == id) {
                let _ = SendMessageW(self.history.list, LB_SETCURSEL, index, 0);
            } else if !self.history_rows.is_empty() {
                let _ = SendMessageW(self.history.list, LB_SETCURSEL, 0, 0);
            }
        } else if !self.history_rows.is_empty() {
            let _ = SendMessageW(self.history.list, LB_SETCURSEL, 0, 0);
        }

        if preserve_scroll && top_index >= 0 {
            let _ = SendMessageW(self.history.list, LB_SETTOPINDEX, top_index as usize, 0);
        }
        self.update_history_hotkey_label();
        self.update_history_detail_label();
        EnableWindow(
            self.history.delete_button,
            self.selected_history_id().is_some() as i32,
        );
    }

    unsafe fn update_history_hotkey_label(&self) {
        if self.history.hotkey == 0 {
            return;
        }

        let label = format!(
            "快捷键: {}",
            format_hotkey_for_display(self.controller.hotkey())
        );
        self.set_window_text(self.history.hotkey, &label);
    }

    unsafe fn update_history_detail_label(&self) {
        if self.history.detail == 0 {
            return;
        }

        let detail = self
            .selected_history_row()
            .map(|row| one_line_text(&row.detail_tooltip))
            .or_else(|| self.controller.settings_snapshot().status)
            .unwrap_or_default();
        self.set_window_text(self.history.detail, &detail);
    }

    unsafe fn handle_history_command(&mut self, control_id: i32, notification: u16) {
        match (control_id, notification) {
            (ID_HISTORY_SEARCH, value) if value == EN_CHANGE as u16 => {
                self.refresh_history_list(true)
            }
            (ID_HISTORY_LIST, value) if value == LBN_SELCHANGE as u16 => {
                self.update_history_detail_label()
            }
            (ID_HISTORY_LIST, value) if value == LBN_DBLCLK as u16 => {
                self.activate_selected_history_item()
            }
            (ID_HISTORY_CLEAR, value) if value == BN_CLICKED as u16 => {
                let _ = self.controller.clear_history();
                self.refresh_history_list(false);
            }
            (ID_HISTORY_DELETE, value) if value == BN_CLICKED as u16 => {
                if let Some(id) = self.selected_history_id() {
                    let _ = self.controller.delete_history_item(id);
                    self.refresh_history_list(false);
                }
            }
            (ID_HISTORY_PREFERENCES, value) if value == BN_CLICKED as u16 => {
                self.open_preferences()
            }
            (ID_HISTORY_QUIT, value) if value == BN_CLICKED as u16 => self.quit(),
            _ => {}
        }
    }

    unsafe fn selected_history_id(&self) -> Option<uuid::Uuid> {
        let index = SendMessageW(self.history.list, LB_GETCURSEL, 0, 0);
        if index < 0 {
            return None;
        }
        self.history_rows.get(index as usize).map(|row| row.id)
    }

    unsafe fn selected_history_row(&self) -> Option<&HistoryRow> {
        let index = SendMessageW(self.history.list, LB_GETCURSEL, 0, 0);
        if index < 0 {
            return None;
        }
        self.history_rows.get(index as usize)
    }

    unsafe fn activate_selected_history_item(&mut self) {
        let Some(id) = self.selected_history_id() else {
            return;
        };
        if self.controller.copy_item(id).ok() == Some(true) {
            self.hide_history_popup();
            trigger_immediate_paste(self.previous_foreground);
        }
    }

    unsafe fn open_preferences(&mut self) {
        if self.ensure_preferences_window().is_err() {
            return;
        }

        let _ = self.controller.set_preferences_visible(true);
        self.load_preferences_form_from_controller();
        ShowWindow(self.preferences.hwnd, SW_SHOWNORMAL);
        SetForegroundWindow(self.preferences.hwnd);
        UpdateWindow(self.preferences.hwnd);
    }

    unsafe fn hide_preferences(&mut self) {
        if self.preferences.hwnd != 0 {
            ShowWindow(self.preferences.hwnd, SW_HIDE);
        }
        let _ = self.controller.set_preferences_visible(false);
    }

    unsafe fn load_preferences_form_from_controller(&mut self) {
        let snapshot = self.controller.settings_snapshot();
        let raw = PreferencesRawState {
            device_name: snapshot.device_name.clone(),
            history_limit_text: snapshot.history_limit.to_string(),
            hotkey: snapshot.hotkey.clone(),
            share_local_history: snapshot.share_local_history,
            prefer_remote_latest_on_paste: snapshot.prefer_remote_latest_on_paste,
            discovery_enabled: snapshot.discovery_enabled,
        };

        self.preferences_baseline_raw = Some(raw.clone());
        self.preferences_devices = snapshot.devices.clone();
        self.set_edit_text(self.preferences.device_name_input, &raw.device_name);
        self.set_edit_text(
            self.preferences.history_limit_input,
            &raw.history_limit_text,
        );
        let _ = SendMessageW(
            self.preferences.share_checkbox,
            BM_SETCHECK,
            if raw.share_local_history {
                BST_CHECKED as usize
            } else {
                BST_UNCHECKED as usize
            },
            0,
        );
        let _ = SendMessageW(
            self.preferences.prefer_remote_checkbox,
            BM_SETCHECK,
            if raw.prefer_remote_latest_on_paste {
                BST_CHECKED as usize
            } else {
                BST_UNCHECKED as usize
            },
            0,
        );
        let _ = SendMessageW(
            self.preferences.discovery_checkbox,
            BM_SETCHECK,
            if raw.discovery_enabled {
                BST_CHECKED as usize
            } else {
                BST_UNCHECKED as usize
            },
            0,
        );
        if let Ok(control_value) = hotkey_control_value(&raw.hotkey) {
            let _ = SendMessageW(
                self.preferences.hotkey_input,
                HKM_SETHOTKEY,
                control_value as usize,
                0,
            );
        }
        self.refresh_preferences_devices_and_status();
        self.update_preferences_save_button();
    }

    unsafe fn refresh_preferences_devices_and_status(&mut self) {
        let snapshot = self.controller.settings_snapshot();
        self.preferences_devices = snapshot.devices.clone();
        let selected_id = self.selected_preferences_device_id();
        let _ = SendMessageW(self.preferences.devices_list, LB_RESETCONTENT, 0, 0);
        for device in &self.preferences_devices {
            let line = wide(&format!(
                "{} · {}",
                device.device_name, device.secondary_text
            ));
            let _ = SendMessageW(
                self.preferences.devices_list,
                LB_ADDSTRING,
                0,
                line.as_ptr() as isize,
            );
        }

        if let Some(device_id) = selected_id {
            if let Some(index) = self
                .preferences_devices
                .iter()
                .position(|device| device.device_id == device_id)
            {
                let _ = SendMessageW(self.preferences.devices_list, LB_SETCURSEL, index, 0);
            }
        }

        self.set_window_text(
            self.preferences.status_label,
            snapshot.status.as_deref().unwrap_or(""),
        );
        self.update_preferences_device_buttons();
    }

    unsafe fn selected_preferences_device_id(&self) -> Option<String> {
        let index = SendMessageW(self.preferences.devices_list, LB_GETCURSEL, 0, 0);
        if index < 0 {
            return None;
        }
        self.preferences_devices
            .get(index as usize)
            .map(|device| device.device_id.clone())
    }

    unsafe fn update_preferences_device_buttons(&self) {
        let selected = self.selected_preferences_device_id().and_then(|id| {
            self.preferences_devices
                .iter()
                .find(|device| device.device_id == id)
        });

        let trust_enabled = selected.map(|device| !device.is_trusted).unwrap_or(false);
        let revoke_enabled = selected.map(|device| device.is_trusted).unwrap_or(false);
        EnableWindow(self.preferences.trust_button, trust_enabled as i32);
        EnableWindow(self.preferences.revoke_button, revoke_enabled as i32);
    }

    unsafe fn update_preferences_tab_visibility(&self) {
        let current = SendMessageW(self.preferences.tab, TCM_GETCURSEL, 0, 0) as i32;
        let show_general = if current < 0 { TAB_GENERAL } else { current } == TAB_GENERAL;
        let show_sharing = if current < 0 { TAB_GENERAL } else { current } == TAB_SHARING;
        let show_hotkey = if current < 0 { TAB_GENERAL } else { current } == TAB_HOTKEY;

        for hwnd in [
            self.preferences.device_name_label,
            self.preferences.device_name_input,
            self.preferences.history_limit_label,
            self.preferences.history_limit_input,
        ] {
            ShowWindow(hwnd, if show_general { SW_SHOW } else { SW_HIDE });
        }

        for hwnd in [
            self.preferences.share_checkbox,
            self.preferences.prefer_remote_checkbox,
            self.preferences.discovery_checkbox,
            self.preferences.devices_label,
            self.preferences.devices_list,
            self.preferences.trust_button,
            self.preferences.revoke_button,
        ] {
            ShowWindow(hwnd, if show_sharing { SW_SHOW } else { SW_HIDE });
        }

        for hwnd in [self.preferences.hotkey_label, self.preferences.hotkey_input] {
            ShowWindow(hwnd, if show_hotkey { SW_SHOW } else { SW_HIDE });
        }
    }

    unsafe fn handle_preferences_command(&mut self, control_id: i32, notification: u16) {
        match (control_id, notification) {
            (ID_PREFS_DEVICE_NAME, value) if value == EN_CHANGE as u16 => {
                self.update_preferences_save_button()
            }
            (ID_PREFS_HISTORY_LIMIT, value) if value == EN_CHANGE as u16 => {
                self.update_preferences_save_button()
            }
            (ID_PREFS_SHARE_LOCAL, value) if value == BN_CLICKED as u16 => {
                self.update_preferences_save_button()
            }
            (ID_PREFS_PREFER_REMOTE, value) if value == BN_CLICKED as u16 => {
                self.update_preferences_save_button()
            }
            (ID_PREFS_DISCOVERY, value) if value == BN_CLICKED as u16 => {
                self.update_preferences_save_button()
            }
            (ID_PREFS_DEVICES_LIST, value) if value == LBN_SELCHANGE as u16 => {
                self.update_preferences_device_buttons()
            }
            (ID_PREFS_TRUST, value) if value == BN_CLICKED as u16 => {
                if let Some(device_id) = self.selected_preferences_device_id() {
                    let _ = self.controller.trust_device(&device_id);
                    self.refresh_preferences_devices_and_status();
                }
            }
            (ID_PREFS_REVOKE, value) if value == BN_CLICKED as u16 => {
                if let Some(device_id) = self.selected_preferences_device_id() {
                    let _ = self.controller.revoke_device_trust(&device_id);
                    self.refresh_preferences_devices_and_status();
                }
            }
            (ID_PREFS_SAVE, value) if value == BN_CLICKED as u16 => self.save_preferences(),
            (ID_PREFS_CLOSE, value) if value == BN_CLICKED as u16 => self.hide_preferences(),
            _ => {}
        }
    }

    unsafe fn save_preferences(&mut self) {
        match self.current_preferences_update() {
            Ok(update) => {
                if let Err(error) = self.controller.apply_settings(update) {
                    self.set_window_text(self.preferences.status_label, &error.to_string());
                    return;
                }
                let _ = self.refresh_global_hotkey();
                self.load_preferences_form_from_controller();
            }
            Err(error) => self.set_window_text(self.preferences.status_label, &error),
        }
    }

    unsafe fn update_preferences_save_button(&self) {
        let dirty = self
            .preferences_baseline_raw
            .as_ref()
            .map(|baseline| self.current_preferences_raw_state() != *baseline)
            .unwrap_or(false);
        let valid = self.current_preferences_update().is_ok();
        EnableWindow(self.preferences.save_button, (dirty && valid) as i32);
    }

    unsafe fn current_preferences_raw_state(&self) -> PreferencesRawState {
        PreferencesRawState {
            device_name: self.read_window_text(self.preferences.device_name_input),
            history_limit_text: self.read_window_text(self.preferences.history_limit_input),
            hotkey: self
                .current_hotkey_value()
                .unwrap_or_else(|_| self.controller.hotkey().to_string()),
            share_local_history: self.checkbox_checked(self.preferences.share_checkbox),
            prefer_remote_latest_on_paste: self
                .checkbox_checked(self.preferences.prefer_remote_checkbox),
            discovery_enabled: self.checkbox_checked(self.preferences.discovery_checkbox),
        }
    }

    unsafe fn current_preferences_update(&self) -> Result<SettingsUpdate, String> {
        let history_limit_text = self.read_window_text(self.preferences.history_limit_input);
        let history_limit = history_limit_text
            .trim()
            .parse::<usize>()
            .map_err(|_| "历史记录数量必须是有效整数。".to_string())?;

        let update = SettingsUpdate {
            device_name: self.read_window_text(self.preferences.device_name_input),
            history_limit,
            hotkey: self.current_hotkey_value()?,
            launch_at_login: self.controller.settings_snapshot().launch_at_login,
            share_local_history: self.checkbox_checked(self.preferences.share_checkbox),
            prefer_remote_latest_on_paste: self
                .checkbox_checked(self.preferences.prefer_remote_checkbox),
            discovery_enabled: self.checkbox_checked(self.preferences.discovery_checkbox),
        };
        self.controller
            .validate_settings_update(&update)
            .map_err(|error| error.to_string())?;
        Ok(update)
    }

    unsafe fn current_hotkey_value(&self) -> Result<String, String> {
        let value = SendMessageW(self.preferences.hotkey_input, HKM_GETHOTKEY, 0, 0) as u32;
        hotkey_from_control_value(value)
    }

    unsafe fn refresh_global_hotkey(&mut self) -> AppResult<()> {
        if self.registered_hotkey.take().is_some() && self.main_hwnd != 0 {
            let _ = UnregisterHotKey(self.main_hwnd, GLOBAL_HOTKEY_ID);
        }

        let registered =
            parse_registered_hotkey(self.controller.hotkey()).map_err(AppError::InvalidConfig)?;
        if RegisterHotKey(
            self.main_hwnd,
            GLOBAL_HOTKEY_ID,
            registered.modifiers,
            registered.vkey,
        ) == 0
        {
            return Err(AppError::InvalidConfig(format!(
                "Failed to register the Windows global hotkey: {}",
                std::io::Error::last_os_error()
            )));
        }

        self.registered_hotkey = Some(registered);
        self.update_history_hotkey_label();
        Ok(())
    }

    unsafe fn show_about(&self) {
        let title = wide("关于");
        let body = wide(&format!("{}\n版本 {}", APP_NAME, env!("CARGO_PKG_VERSION")));
        let _ = MessageBoxW(
            self.main_hwnd,
            body.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }

    unsafe fn quit(&mut self) {
        let _ = DestroyWindow(self.main_hwnd);
    }

    unsafe fn read_window_text(&self, hwnd: HWND) -> String {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }

        let mut buffer = vec![0_u16; len as usize + 1];
        let copied = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..copied as usize])
    }

    unsafe fn set_window_text(&self, hwnd: HWND, value: &str) {
        let value = wide(value);
        let _ = SetWindowTextW(hwnd, value.as_ptr());
    }

    unsafe fn set_edit_text(&self, hwnd: HWND, value: &str) {
        self.set_window_text(hwnd, value);
    }

    unsafe fn checkbox_checked(&self, hwnd: HWND) -> bool {
        SendMessageW(hwnd, BM_GETCHECK, 0, 0) as u32 == BST_CHECKED
    }
}

unsafe extern "system" fn app_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        let app = create.lpCreateParams as *mut WindowsApp;
        let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as isize);
        return 1;
    }

    let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowsApp;
    if !app.is_null() && message != WM_NCDESTROY {
        return (*app).window_proc(hwnd, message, wparam, lparam);
    }

    DefWindowProcW(hwnd, message, wparam, lparam)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn copy_wide_into_fixed<const N: usize>(wide_text: &[u16], fixed: &mut [u16; N]) {
    let count = wide_text.len().min(N);
    fixed[..count].copy_from_slice(&wide_text[..count]);
    if count < N {
        fixed[count] = 0;
    } else if let Some(last) = fixed.last_mut() {
        *last = 0;
    }
}

fn loword(value: usize) -> u16 {
    (value & 0xFFFF) as u16
}

fn hiword(value: usize) -> u16 {
    ((value >> 16) & 0xFFFF) as u16
}

fn one_line_text(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(value)
        .to_string()
}
