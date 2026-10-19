// アプリ定義の Tauri command を AppManifest へ宣言し、command ごとの権限を自動生成させる
// （タスク vA1wHU / 判断台帳 D110）。宣言した command は capability で許可されたウィンドウからしか呼べない。
// 一覧は command_manifest.rs に置き、`cargo test` の command_permissions テストと共有する。
include!("command_manifest.rs");

fn main() {
    // include! した一覧の変更でも build script を再実行させる（tauri_build が rerun-if-changed を出すため明示する）。
    println!("cargo:rerun-if-changed=command_manifest.rs");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
