//! マイク使用中・会議中かどうかを OS に問い合わせる（ゆうこ通知の抑制判定用）。
//!
//! 設計書（ゆうこ登場・通知挙動 §5.2）の「マイク使用中」「会議中」に対応する（判断台帳 D65 / D68 / D69）。
//! - マイク使用中: Windows のプライバシー設定（CapabilityAccessManager の ConsentStore）に
//!   「現在マイクを使用中」のアプリが記録されているか（D68 A）。
//! - 会議アプリ起動中: 既知の会議アプリの実行ファイル名を持つプロセスが存在するか（D69 A）。
//!
//! プライバシー: どのアプリがマイクを使っているか・どのプロセスが動いているかは保持・表示・ログ出力せず、
//! 真偽値だけを返す。常時監視はせず、通知判定のたびに呼び出し側が必要な分だけ問い合わせる。
//! 判定ロジックを Windows なしでテストできるよう、OS 依存部分はこの trait の実装だけに閉じ込める。
//! Windows 以外では判定を無効化し、常に false（抑制しない）を返す。

use std::fmt::Debug;

/// 会議アプリとみなす実行ファイル名（大文字小文字は区別しない）。
///
/// - `ms-teams.exe`: 新しい Microsoft Teams / `Teams.exe`: 従来版 Teams
/// - `Zoom.exe`: Zoom デスクトップクライアント
/// - `CiscoWebexStart.exe` / `webexmta.exe` / `atmgr.exe`: Cisco Webex の起動・会議プロセス
///
/// ブラウザで参加する会議（Chrome 上の Google Meet など）はプロセス名で区別できないため検知しない。
/// その場合でも、設定「マイク使用中は通知を抑制する」が ON ならマイク使用中として抑制される。
pub const MEETING_APP_EXECUTABLES: &[&str] = &[
    "ms-teams.exe",
    "Teams.exe",
    "Zoom.exe",
    "CiscoWebexStart.exe",
    "webexmta.exe",
    "atmgr.exe",
];

/// マイク使用中・会議アプリ起動中の判定境界。テストでは固定値を返す実装を差し込む。
///
/// どちらも問い合わせに失敗した場合は false（抑制しない・fail-open）を返す。
pub trait MeetingDetector: Debug + Send + Sync {
    /// いずれかのアプリが現在マイクを使用中か。
    fn is_mic_in_use(&self) -> bool;
    /// 既知の会議アプリ（[`MEETING_APP_EXECUTABLES`]）のプロセスが存在するか。
    fn is_meeting_app_running(&self) -> bool;
}

/// 実行環境の OS に問い合わせる実装。
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemMeetingDetector;

impl MeetingDetector for SystemMeetingDetector {
    #[cfg(windows)]
    fn is_mic_in_use(&self) -> bool {
        windows_impl::any_app_using_microphone()
    }

    /// Windows 以外は判定手段を持たないため、抑制しない側を返す。
    #[cfg(not(windows))]
    fn is_mic_in_use(&self) -> bool {
        false
    }

    #[cfg(windows)]
    fn is_meeting_app_running(&self) -> bool {
        windows_impl::any_meeting_app_running()
    }

    /// Windows 以外は判定手段を持たないため、抑制しない側を返す。
    #[cfg(not(windows))]
    fn is_meeting_app_running(&self) -> bool {
        false
    }
}

/// ConsentStore の 1 アプリ分の記録（LastUsedTimeStart / LastUsedTimeStop の QWORD）から、
/// 現在マイクを使用中かを判定する。
///
/// Windows は使用開始時に Start を記録して Stop を 0 にし、使用終了時に Stop を記録する。
/// 一度も使っていないアプリは Start も 0 のため、「Start が 0 以外で Stop が 0」だけを使用中とする。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn is_usage_active(last_used_start: u64, last_used_stop: u64) -> bool {
    last_used_start != 0 && last_used_stop == 0
}

/// 実行ファイル名が既知の会議アプリか（大文字小文字を区別しない）。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn is_meeting_executable(executable_name: &str) -> bool {
    MEETING_APP_EXECUTABLES
        .iter()
        .any(|known| known.eq_ignore_ascii_case(executable_name))
}

#[cfg(windows)]
mod windows_impl {
    use super::{is_meeting_executable, is_usage_active};
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
        RRF_RT_REG_QWORD,
    };

    /// マイクの利用記録（現在のユーザー）。直下のサブキーがパッケージアプリ、
    /// `NonPackaged` 配下のサブキーがデスクトップアプリ（実行ファイルのパスを '#' 区切りにした名前）。
    const MICROPHONE_CONSENT_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    const NON_PACKAGED_SUBKEY: &str = "NonPackaged";

    /// レジストリのキー名は最大 255 文字（終端 NUL を含めて 256）。
    const MAX_KEY_NAME_LEN: usize = 256;
    /// 想定外に多いサブキーで判定が長引かないための上限。
    const MAX_ENUMERATED_SUBKEYS: u32 = 4096;

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 読み取り専用で開いたレジストリキー。Drop で必ず閉じる。
    struct RegKey(HKEY);

    impl RegKey {
        fn open(parent: HKEY, subkey: &str) -> Option<Self> {
            let subkey = wide(subkey);
            let mut handle: HKEY = std::ptr::null_mut();
            // SAFETY: subkey は NUL 終端の UTF-16、handle は出力先として有効なポインタ。
            let status =
                unsafe { RegOpenKeyExW(parent, subkey.as_ptr(), 0, KEY_READ, &mut handle) };
            (status == ERROR_SUCCESS).then_some(Self(handle))
        }
    }

    impl Drop for RegKey {
        fn drop(&mut self) {
            // SAFETY: open で成功したハンドルだけを保持しており、ここで 1 回だけ閉じる。
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }

    /// サブキー `subkey`（NUL 終端の UTF-16）の QWORD 値を読む。無い・型違いは None。
    fn read_qword(key: &RegKey, subkey: &[u16], value_name: &str) -> Option<u64> {
        let value_name = wide(value_name);
        let mut data: u64 = 0;
        let mut size = std::mem::size_of::<u64>() as u32;
        // SAFETY: 文字列は NUL 終端、data は size バイトの書き込み可能領域。
        let status = unsafe {
            RegGetValueW(
                key.0,
                subkey.as_ptr(),
                value_name.as_ptr(),
                RRF_RT_REG_QWORD,
                std::ptr::null_mut(),
                (&mut data as *mut u64).cast(),
                &mut size,
            )
        };
        (status == ERROR_SUCCESS).then_some(data)
    }

    /// `key` 直下のサブキーのうち、マイクを現在使用中のものが 1 つでもあれば true。
    /// 見つかった時点で列挙をやめる。サブキー名（アプリ名・パス）は保持もログ出力もしない。
    fn any_subkey_in_use(key: &RegKey) -> bool {
        let mut name = [0u16; MAX_KEY_NAME_LEN];
        for index in 0..MAX_ENUMERATED_SUBKEYS {
            let mut len = MAX_KEY_NAME_LEN as u32;
            // SAFETY: name は len 文字分の書き込み可能領域。不要な出力（クラス名・更新時刻）は null。
            let status = unsafe {
                RegEnumKeyExW(
                    key.0,
                    index,
                    name.as_mut_ptr(),
                    &mut len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                return false;
            }
            if status != ERROR_SUCCESS {
                // 名前が長すぎる等の個別の失敗は、そのサブキーだけ飛ばす。
                continue;
            }
            // 成功時、len は終端 NUL を含まない文字数。RegGetValueW 用に NUL 終端を保証する。
            let len = (len as usize).min(MAX_KEY_NAME_LEN - 1);
            name[len] = 0;
            let subkey = &name[..=len];
            let (Some(start), Some(stop)) = (
                read_qword(key, subkey, "LastUsedTimeStart"),
                read_qword(key, subkey, "LastUsedTimeStop"),
            ) else {
                continue;
            };
            if is_usage_active(start, stop) {
                return true;
            }
        }
        false
    }

    /// パッケージアプリ・デスクトップアプリのどちらかが現在マイクを使用中か。
    /// キーが無い（一度もマイクが使われていない）・開けない場合は false。
    pub(super) fn any_app_using_microphone() -> bool {
        let Some(root) = RegKey::open(HKEY_CURRENT_USER, MICROPHONE_CONSENT_KEY) else {
            return false;
        };
        if any_subkey_in_use(&root) {
            return true;
        }
        RegKey::open(root.0, NON_PACKAGED_SUBKEY).is_some_and(|key| any_subkey_in_use(&key))
    }

    /// 既知の会議アプリのプロセスが 1 つでもあれば true。プロセス一覧は保持もログ出力もしない。
    pub(super) fn any_meeting_app_running() -> bool {
        // SAFETY: フラグと 0（全プロセス）を渡すだけ。戻り値は下で検証して必ず閉じる。
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            log::warn!("プロセス一覧を取得できなかったため、会議中による通知抑制を行いません");
            return false;
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        // SAFETY: snapshot は有効なハンドル、entry は dwSize を設定した書き込み可能な構造体。
        let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while has_entry {
            let len = entry
                .szExeFile
                .iter()
                .position(|&ch| ch == 0)
                .unwrap_or(entry.szExeFile.len());
            if is_meeting_executable(&String::from_utf16_lossy(&entry.szExeFile[..len])) {
                found = true;
                break;
            }
            // SAFETY: 同上。
            has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        // SAFETY: CreateToolhelp32Snapshot で得た有効なハンドルを 1 回だけ閉じる。
        unsafe {
            CloseHandle(snapshot);
        }
        found
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// 実機の状態に依存せず、OS 呼び出しが panic・異常終了しないことだけを確認する。
        #[test]
        fn system_queries_complete_without_panicking() {
            let _ = any_app_using_microphone();
            let _ = any_meeting_app_running();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_is_active_only_when_started_and_not_stopped() {
        assert!(is_usage_active(133_000_000_000_000_000, 0));
        assert!(!is_usage_active(
            133_000_000_000_000_000,
            133_000_000_100_000_000
        ));
        // 一度も使っていない（両方 0）は使用中ではない。
        assert!(!is_usage_active(0, 0));
        assert!(!is_usage_active(0, 133_000_000_100_000_000));
    }

    #[test]
    fn known_meeting_executables_match_case_insensitively() {
        for name in [
            "ms-teams.exe",
            "MS-TEAMS.EXE",
            "Teams.exe",
            "teams.exe",
            "Zoom.exe",
            "zoom.exe",
            "CiscoWebexStart.exe",
            "WebexMTA.exe",
            "atmgr.exe",
        ] {
            assert!(is_meeting_executable(name), "{name}");
        }
    }

    #[test]
    fn other_executables_are_not_meeting_apps() {
        for name in [
            "chrome.exe",
            "msedge.exe",
            "Zoom",
            "zoom.exe.bak",
            "teams",
            "",
        ] {
            assert!(!is_meeting_executable(name), "{name}");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_detection_is_disabled() {
        let detector = SystemMeetingDetector;
        assert!(!detector.is_mic_in_use());
        assert!(!detector.is_meeting_app_running());
    }
}
