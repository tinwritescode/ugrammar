//! Words the Rust side shows the user (popover messages, notifications, menus, the welcome dialog).
//! The web UI has its own table in `src/renderer/i18n.ts`.
use serde::{Deserialize, Serialize};

/// The UI language the user picked on first launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Vi,
}

impl Lang {
    /// Value for `explainLang`, so per-fix reasons start in the same language.
    pub fn explain_lang(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Vi => "Vietnamese",
        }
    }
}

/// One field per message, so a language missing a message does not compile.
/// `{name}` marks a value filled in with [`fill`].
pub struct Strings {
    pub checking: &'static str,
    pub skip_excluded: &'static str,
    pub skip_secure: &'static str,
    pub select_first: &'static str,
    /// {code}
    pub read_failed: &'static str,
    pub focus_changed: &'static str,
    /// {code}
    pub write_failed: &'static str,
    pub grant_ax: &'static str,
    pub needs_ax: &'static str,
    pub too_short: &'static str,
    /// {msg}
    pub check_failed: &'static str,
    pub engine_error: &'static str,
    /// A trailing ✓ marks a success message for the popover.
    pub no_errors: &'static str,
    pub applied: &'static str,
    /// {hotkey}
    pub auto_write_failed: &'static str,
    /// {error}
    pub hotkey_failed: &'static str,
    /// {version}
    pub update_installed: &'static str,
    pub ready: &'static str,
    pub checking_status: &'static str,
    pub paused: &'static str,
    pub error: &'static str,
    pub needs_permission: &'static str,
    /// {status}
    pub menu_status: &'static str,
    /// {hotkey}
    pub menu_fix: &'static str,
    pub menu_pause: &'static str,
    pub menu_ax: &'static str,
    pub menu_settings: &'static str,
    pub menu_quit: &'static str,
    pub rewrite_as: &'static str,
    pub formal: &'static str,
    pub friendly: &'static str,
    pub short: &'static str,
    pub welcome_title: &'static str,
    /// {hotkey} {endpoint}
    pub welcome_body: &'static str,
    pub get_started: &'static str,
}

const EN: Strings = Strings {
    checking: "Checking…",
    skip_excluded: "Skipped: this app is on the excluded list.",
    skip_secure: "Skipped: password or secure field.",
    select_first: "Select some text first.",
    read_failed: "Could not read the selection ({code}).",
    focus_changed: "Focus changed; text not replaced.",
    write_failed: "Could not replace text ({code}).",
    grant_ax: "Grant Accessibility permission",
    needs_ax: "ugrammar needs Accessibility permission (System Settings → Privacy & Security → Accessibility).",
    too_short: "Too short to check (under 3 words).",
    check_failed: "Check failed: {msg}. Your text was not changed.",
    engine_error: "Engine error",
    no_errors: "No mistakes ✓",
    applied: "Fixed, ⌘Z to undo ✓",
    auto_write_failed: "Could not fix it. Select the sentence and press {hotkey}",
    hotkey_failed: "Could not register hotkey ({error}). It may be in use by another app.",
    update_installed: "Version {version} is installed. Restart ugrammar from the menu bar to use it.",
    ready: "Ready",
    checking_status: "Checking…",
    paused: "Paused",
    error: "Error",
    needs_permission: "Needs Accessibility permission",
    menu_status: "Status: {status}",
    menu_fix: "Fix selection: {hotkey}",
    menu_pause: "Pause",
    menu_ax: "Open Accessibility Settings…",
    menu_settings: "Settings…",
    menu_quit: "Quit ugrammar",
    rewrite_as: "Rewrite selection as…",
    formal: "Formal",
    friendly: "Friendly",
    short: "Short",
    welcome_title: "Welcome to ugrammar",
    welcome_body: "1. Grant Accessibility permission when macOS asks (System Settings → Privacy & Security → Accessibility). Needed to read and replace text.\n\
                   2. Select text and press {hotkey} to proofread. ⌥⌘T rewrites in a tone.\n\
                   3. Click the ug icon in the menu bar for everything else: auto-check, tone, engine, dictionary.\n\n\
                   Privacy: checked text is sent to {endpoint} (change it from the ug menu-bar panel). Password fields and excluded apps are never read. Text is never logged or stored.",
    get_started: "Get started",
};

const VI: Strings = Strings {
    checking: "Đang kiểm tra…",
    skip_excluded: "Bỏ qua: app này nằm trong danh sách bỏ qua.",
    skip_secure: "Bỏ qua: ô mật khẩu hoặc ô bảo mật.",
    select_first: "Hãy chọn chữ trước.",
    read_failed: "Không đọc được đoạn đã chọn ({code}).",
    focus_changed: "Đã chuyển sang ô khác, chữ chưa được thay.",
    write_failed: "Không thay được chữ ({code}).",
    grant_ax: "Cần cấp quyền Accessibility",
    needs_ax: "ugrammar cần quyền Accessibility (Cài đặt hệ thống → Quyền riêng tư & Bảo mật → Trợ năng).",
    too_short: "Quá ngắn để kiểm tra (dưới 3 từ).",
    check_failed: "Kiểm tra thất bại: {msg}. Chữ của bạn chưa bị đổi.",
    engine_error: "Lỗi engine",
    no_errors: "Không có lỗi nào ✓",
    applied: "Đã sửa, ⌘Z để hoàn tác ✓",
    auto_write_failed: "Không sửa được, hãy chọn câu rồi bấm {hotkey}",
    hotkey_failed: "Không đăng ký được phím tắt ({error}). Có thể app khác đang dùng.",
    update_installed: "Đã cài bản {version}. Khởi động lại ugrammar từ thanh menu để dùng.",
    ready: "Sẵn sàng",
    checking_status: "Đang kiểm tra…",
    paused: "Đã tạm dừng",
    error: "Có lỗi",
    needs_permission: "Cần quyền Accessibility",
    menu_status: "Trạng thái: {status}",
    menu_fix: "Sửa đoạn đã chọn: {hotkey}",
    menu_pause: "Tạm dừng",
    menu_ax: "Mở cài đặt Trợ năng…",
    menu_settings: "Cài đặt…",
    menu_quit: "Thoát ugrammar",
    rewrite_as: "Viết lại đoạn đã chọn…",
    formal: "Trang trọng",
    friendly: "Thân thiện",
    short: "Ngắn gọn",
    welcome_title: "Chào mừng đến với ugrammar",
    welcome_body: "1. Cấp quyền Accessibility khi macOS hỏi (Cài đặt hệ thống → Quyền riêng tư & Bảo mật → Trợ năng). Cần để đọc và thay chữ.\n\
                   2. Chọn chữ rồi bấm {hotkey} để sửa. ⌥⌘T viết lại theo giọng văn.\n\
                   3. Bấm biểu tượng ug trên thanh menu cho mọi thứ khác: tự kiểm tra, giọng văn, engine, từ điển.\n\n\
                   Riêng tư: chữ cần kiểm tra được gửi tới {endpoint} (đổi trong bảng ug trên thanh menu). Ô mật khẩu và app bỏ qua không bao giờ bị đọc. Chữ không bao giờ được ghi log hay lưu lại.",
    get_started: "Bắt đầu",
};

pub fn strings(lang: Lang) -> &'static Strings {
    match lang {
        Lang::En => &EN,
        Lang::Vi => &VI,
    }
}

/// Replaces each `{name}` in `template` with its value.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(template.to_string(), |s, (k, v)| s.replace(&format!("{{{k}}}"), v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_named_values() {
        assert_eq!(fill(strings(Lang::Vi).auto_write_failed, &[("hotkey", "⌥⌘G")]), "Không sửa được, hãy chọn câu rồi bấm ⌥⌘G");
        assert_eq!(fill(strings(Lang::En).menu_fix, &[("hotkey", "⌥⌘G")]), "Fix selection: ⌥⌘G");
    }

    #[test]
    fn success_messages_keep_the_trailing_check_mark() {
        for lang in [Lang::En, Lang::Vi] {
            assert!(strings(lang).no_errors.ends_with('✓') && strings(lang).applied.ends_with('✓'));
        }
    }
}
