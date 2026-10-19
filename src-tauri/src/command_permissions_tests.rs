//! Tauri command のウィンドウごとの許可制（タスク vA1wHU / 判断台帳 D110）の整合性テスト。
//!
//! command は次の 4 か所に現れ、どれかがずれると「呼べない画面」や「余計な権限」が生まれる。
//!   - src/lib.rs の `generate_handler!`（実際に登録される command）
//!   - command_manifest.rs の `APP_COMMANDS`（build.rs が AppManifest へ宣言し、権限を自動生成する）
//!   - permissions/main-window.toml の `main-window-commands`（メインウィンドウへ一括で許可する権限セット）
//!   - capabilities/yuuko-window.json（常駐ゆうこウィンドウへ許可する command）
//!
//! Tauri は宣言漏れ・許可漏れをビルドでは検出せず、実行時に呼び出しを拒否するだけなので、
//! ここで突き合わせて CI（`cargo test`）で止める。
//! `generate_handler!` はマクロで command の一覧を値として取り出せないため、lib.rs のソースを
//! `include_str!` で読み、マクロ呼び出し部分だけを文字列として解析する。

use std::collections::BTreeSet;

include!("../command_manifest.rs");

const LIB_RS: &str = include_str!("lib.rs");
const MAIN_WINDOW_PERMISSIONS: &str = include_str!("../permissions/main-window.toml");
const MAIN_CAPABILITY: &str = include_str!("../capabilities/default.json");
const YUUKO_CAPABILITY: &str = include_str!("../capabilities/yuuko-window.json");

/// 常駐ゆうこウィンドウに許可してよい command（判断台帳 D110・D114）。広げる場合は判断を取り直す。
/// 通知の表示・反応に使う 4 つと、配色だけに使うテーマ ID の取得（設定全体の `get_user_settings` は不可）。
const YUUKO_WINDOW_COMMANDS: &[&str] = &[
    "get_yuuko_notification_state",
    "handle_yuuko_clicked",
    "dismiss_yuuko_notification",
    "mark_yuuko_ignored",
    "get_ui_theme_id",
];

const MAIN_WINDOW_SET: &str = "main-window-commands";

/// `generate_handler![ ... ]` に並ぶ command 名（パスの最後の要素）を取り出す。
/// 前提: lib.rs の `generate_handler!` は 1 か所だけで、`])` で閉じる（`.invoke_handler(...)` の閉じ括弧と続く）。
/// 範囲は最初の `])` までとするため、要素の途中に `])` を書かないこと（属性 `#[cfg(...)]` 付きの要素も想定しない）。
/// 書き方を変えて解析できなくなった場合は、件数・一致の検証で失敗するので、ここを合わせて直す。
fn registered_commands() -> Vec<String> {
    let start = LIB_RS
        .find("generate_handler![")
        .expect("lib.rs に generate_handler! が見つからない")
        + "generate_handler![".len();
    let len = LIB_RS[start..]
        .find("])")
        .expect("generate_handler! の閉じ括弧 `])` が見つからない");
    LIB_RS[start..start + len]
        .lines()
        // 行末コメントを除いてから、カンマ区切りの各要素を読む。
        .map(|line| line.split("//").next().unwrap_or_default())
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.rsplit("::").next().unwrap_or(entry).to_string())
        .collect()
}

/// 文字列中の `"allow-..."` をすべて取り出す（TOML・JSON の配列要素を想定した簡易抽出）。
fn quoted_allow_permissions(source: &str) -> Vec<String> {
    source
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|value| value.starts_with("allow-"))
        .map(str::to_string)
        .collect()
}

fn allow_permission(command: &str) -> String {
    format!("allow-{}", command.replace('_', "-"))
}

fn capability_permissions(source: &str) -> Vec<String> {
    let value: serde_json::Value =
        serde_json::from_str(source).expect("capability JSON を解析できない");
    value["permissions"]
        .as_array()
        .expect("capability に permissions 配列が無い")
        .iter()
        .map(|permission| {
            permission
                .as_str()
                .expect("permissions は文字列だけを想定")
                .to_string()
        })
        .collect()
}

fn assert_no_duplicates(label: &str, items: &[String]) {
    let unique: BTreeSet<_> = items.iter().collect();
    assert_eq!(unique.len(), items.len(), "{label} に重複がある: {items:?}");
}

#[test]
fn generate_handler_and_app_manifest_list_the_same_commands() {
    let registered = registered_commands();
    assert!(
        registered.len() >= YUUKO_WINDOW_COMMANDS.len(),
        "generate_handler! の解析結果が少なすぎる: {registered:?}"
    );
    assert_no_duplicates("generate_handler!", &registered);

    let manifest: Vec<String> = APP_COMMANDS.iter().map(|c| c.to_string()).collect();
    assert_no_duplicates("command_manifest.rs", &manifest);

    let registered: BTreeSet<_> = registered.into_iter().collect();
    let manifest: BTreeSet<_> = manifest.into_iter().collect();
    assert_eq!(
        registered.difference(&manifest).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "generate_handler! にあって command_manifest.rs に無い command（AppManifest へ宣言する）"
    );
    assert_eq!(
        manifest.difference(&registered).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "command_manifest.rs にあって generate_handler! に無い command（登録漏れか一覧の消し忘れ）"
    );
}

#[test]
fn main_window_set_allows_every_declared_command_and_nothing_else() {
    let in_set = quoted_allow_permissions(MAIN_WINDOW_PERMISSIONS);
    assert_no_duplicates("permissions/main-window.toml", &in_set);

    let expected: BTreeSet<String> = APP_COMMANDS.iter().map(|c| allow_permission(c)).collect();
    let actual: BTreeSet<String> = in_set.into_iter().collect();
    assert_eq!(
        expected.difference(&actual).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "main-window-commands に無い command（メイン画面から呼ぶと実行時に拒否される）"
    );
    assert_eq!(
        actual.difference(&expected).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "main-window-commands にあるが宣言されていない command"
    );
}

#[test]
fn main_window_capability_uses_the_bundled_set() {
    let permissions = capability_permissions(MAIN_CAPABILITY);
    assert!(
        permissions.iter().any(|p| p == MAIN_WINDOW_SET),
        "capabilities/default.json が {MAIN_WINDOW_SET} を参照していない"
    );
    // 個別の allow-* を直書きすると一括セットとの二重管理になるため、セット経由に限る。
    assert_eq!(
        quoted_allow_permissions(MAIN_CAPABILITY),
        Vec::<String>::new(),
        "capabilities/default.json にはアプリ command を個別に書かない"
    );
}

#[test]
fn yuuko_window_capability_allows_only_notification_and_theme_commands() {
    let permissions = capability_permissions(YUUKO_CAPABILITY);
    assert!(
        !permissions.iter().any(|p| p == MAIN_WINDOW_SET),
        "常駐ゆうこウィンドウへ全 command の権限セットを与えてはいけない"
    );

    let app_permissions: BTreeSet<String> = permissions
        .into_iter()
        // `core:` などプラグイン権限は名前空間付き。アプリ定義の権限は名前空間なし。
        .filter(|p| !p.contains(':'))
        .collect();
    let expected: BTreeSet<String> = YUUKO_WINDOW_COMMANDS
        .iter()
        .map(|c| allow_permission(c))
        .collect();
    assert_eq!(app_permissions, expected);

    // 許可する command はすべて宣言済みであること（未宣言だと権限が生成されずビルドで失敗する）。
    for command in YUUKO_WINDOW_COMMANDS {
        assert!(
            APP_COMMANDS.contains(command),
            "{command} が command_manifest.rs に無い"
        );
    }
}

/// ビルド時に解決された実際の ACL（`generate_context!` に埋め込まれる RuntimeAuthority）で、
/// 各ウィンドウから各 command を呼べるかを確かめる。上のテストは設定ファイル同士の突き合わせ、
/// こちらは Tauri が実行時に使う判定そのもの（拒否されると `Command ... not allowed by ACL` になる）。
/// `runtime_authority_mut` は Tauri の非公開扱い（doc(hidden)）の API のため、Tauri 更新で
/// 使えなくなった場合はこのテストだけを見直す（設定ファイルの突き合わせテストは残す）。
#[test]
fn compiled_acl_allows_main_everything_and_yuuko_only_notification_and_theme_commands() {
    let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    let authority = context.runtime_authority_mut();
    let local = tauri::ipc::Origin::Local;

    for command in APP_COMMANDS {
        assert!(
            authority
                .resolve_access(command, "main", "main", &local)
                .is_some(),
            "メインウィンドウから {command} を呼べない"
        );

        let yuuko_allowed = authority
            .resolve_access(command, "yuuko", "yuuko", &local)
            .is_some();
        assert_eq!(
            yuuko_allowed,
            YUUKO_WINDOW_COMMANDS.contains(command),
            "常駐ゆうこウィンドウでの {command} の許可状態が想定と違う"
        );
    }

    // 未知のウィンドウ（ラベル未登録）からはどの command も呼べない。
    for command in APP_COMMANDS {
        assert!(
            authority
                .resolve_access(command, "unknown", "unknown", &local)
                .is_none(),
            "未登録ウィンドウから {command} を呼べてしまう"
        );
    }
}
