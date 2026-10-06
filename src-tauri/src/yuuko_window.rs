//! アプリ非表示中にデスクトップ右下でゆうこ通知を出すための、常駐ゆうこ専用ウィンドウを管理する。
//!
//! 2026-10-05 決定（設計書 §7.3 案B）により、メインウィンドウとは別ラベルの小型ウィンドウを使う。
//! いつ表示するか・何を渡すかは別の判定処理の責務とし、ここでは生成・配置・表示/非表示だけを提供する。
//!
//! ウィンドウは tauri.conf.json で静的に作らず、初回表示時に Rust から遅延生成する。
//! 静的に作ると、未使用時も WebView を常駐させて負荷が増えるうえ、トレイ初期化に失敗して
//! 通常終了へ戻ったときに「非表示のゆうこウィンドウだけが残りプロセスが終わらない」状態になるため。

use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

/// capabilities/yuuko-window.json の windows と一致させる。
pub const YUUKO_WINDOW_LABEL: &str = "yuuko";
/// Next.js の静的エクスポートで out/yuuko-window.html として出力されるプレースホルダページ。
const YUUKO_WINDOW_PAGE: &str = "yuuko-window";
/// ゆうこ本体と吹き出しが収まる大きさ（論理px）。実寸は UI 実装タスクで調整する。
const YUUKO_WINDOW_LOGICAL_WIDTH: f64 = 320.0;
const YUUKO_WINDOW_LOGICAL_HEIGHT: f64 = 360.0;
/// 作業領域の端にぴったり付けると通知領域やタスクバーの境界と重なって見えるため、少し内側へ寄せる。
const SCREEN_EDGE_LOGICAL_MARGIN: f64 = 16.0;

/// ゆうこ用ウィンドウが無ければ、透明・枠なし・最前面・タスクバー非表示・非表示状態で生成する。
///
/// Windows では同期 Tauri command（メインスレッド）内でのウィンドウ生成がデッドロックするため、
/// 呼び出し側はスケジューラスレッドや async command から呼ぶこと。
// 表示タイミングを判定する別タスクから呼ぶまでは未使用になるため、dead_code を許可する。
#[allow(dead_code)]
pub fn ensure_yuuko_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    if let Some(window) = app.get_webview_window(YUUKO_WINDOW_LABEL) {
        return Ok(window);
    }

    let builder = WebviewWindowBuilder::new(
        app,
        YUUKO_WINDOW_LABEL,
        WebviewUrl::App(YUUKO_WINDOW_PAGE.into()),
    )
    .title("ゆうこ")
    .inner_size(YUUKO_WINDOW_LOGICAL_WIDTH, YUUKO_WINDOW_LOGICAL_HEIGHT)
    .visible(false)
    .decorations(false)
    // 枠なし透明ウィンドウに Windows の影が付くと、透明部分の輪郭が見えてしまうため無効化する。
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    // 通知のたびに作業中アプリからフォーカスを奪わないよう、生成時も表示時もアクティブにしない。
    .focused(false)
    .focusable(false);

    // macOS の透明化は macos-private-api 機能が必要で、MVP の対象（Windows）外のため付けない。
    #[cfg(not(target_os = "macos"))]
    let builder = builder.transparent(true);

    builder.build()
}

/// ゆうこ用ウィンドウをプライマリモニタ作業領域の右下へ配置してから表示する。
///
/// 配置先を決められない場合は、意図しない位置に出すより表示しない方が安全なため、表示せずにエラーを返す。
#[allow(dead_code)]
pub fn show_yuuko_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let window = ensure_yuuko_window(app)?;
    place_at_primary_work_area_bottom_right(app, &window)?;
    window.show()?;
    log::info!("ゆうこ用ウィンドウを表示しました");
    Ok(())
}

/// ゆうこ用ウィンドウを隠す。次回表示で再利用するため破棄はしない。
#[allow(dead_code)]
pub fn hide_yuuko_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    match app.get_webview_window(YUUKO_WINDOW_LABEL) {
        Some(window) => window.hide(),
        None => Ok(()),
    }
}

/// メインウィンドウが実際に破棄されるとき、ゆうこ用ウィンドウも閉じる。
///
/// トレイ初期化失敗時の通常終了では「全ウィンドウが閉じたらプロセス終了」に頼っているため、
/// 非表示のゆうこ用ウィンドウが残って終了できなくなるのを防ぐ。
pub fn close_yuuko_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(YUUKO_WINDOW_LABEL) else {
        return;
    };
    if let Err(error) = window.close() {
        log::warn!("ゆうこ用ウィンドウを閉じられませんでした: {error}");
    }
}

fn place_at_primary_work_area_bottom_right<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
) -> tauri::Result<()> {
    let monitor = app.primary_monitor()?.ok_or_else(|| {
        log::warn!("プライマリモニタを取得できないため、ゆうこ用ウィンドウを表示しません");
        tauri::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "primary monitor is not available",
        ))
    })?;

    // 作業領域は物理pxで返るため、ウィンドウ寸法と余白もプライマリモニタの倍率で物理pxへ揃える。
    let scale_factor = monitor.scale_factor();
    let window_size = PhysicalSize::new(
        to_physical(YUUKO_WINDOW_LOGICAL_WIDTH, scale_factor),
        to_physical(YUUKO_WINDOW_LOGICAL_HEIGHT, scale_factor),
    );
    let margin = to_physical(SCREEN_EDGE_LOGICAL_MARGIN, scale_factor);
    let work_area = monitor.work_area();
    let (x, y) = bottom_right_position(
        WorkArea {
            x: work_area.position.x,
            y: work_area.position.y,
            width: work_area.size.width,
            height: work_area.size.height,
        },
        window_size.width,
        window_size.height,
        margin,
    );

    window.set_size(window_size)?;
    window.set_position(PhysicalPosition::new(x, y))?;
    Ok(())
}

fn to_physical(logical: f64, scale_factor: f64) -> u32 {
    let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    (logical * scale_factor).round().max(0.0) as u32
}

/// タスクバーを除いたモニタ作業領域（物理px）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorkArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// 作業領域の右下から余白分だけ内側に、ウィンドウ左上の座標を求める。
///
/// タスクバーが上・左にある場合やマルチモニタで作業領域の原点が負の場合も、作業領域基準で計算する。
/// ウィンドウ（＋余白）が作業領域より大きいときは、右下を優先して左上がはみ出さないよう作業領域の原点へ寄せる。
fn bottom_right_position(
    work_area: WorkArea,
    window_width: u32,
    window_height: u32,
    margin: u32,
) -> (i32, i32) {
    let axis = |origin: i32, area: u32, window: u32| -> i32 {
        let origin = i64::from(origin);
        let preferred = origin + i64::from(area) - i64::from(window) - i64::from(margin);
        let clamped = preferred.max(origin);
        clamped.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    };
    (
        axis(work_area.x, work_area.width, window_width),
        axis(work_area.y, work_area.height, window_height),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x: i32, y: i32, width: u32, height: u32) -> WorkArea {
        WorkArea {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn places_window_above_bottom_taskbar() {
        // 1920x1080 で下端に 40px のタスクバーがある場合、作業領域の高さは 1040。
        assert_eq!(
            bottom_right_position(area(0, 0, 1920, 1040), 320, 360, 16),
            (1920 - 320 - 16, 1040 - 360 - 16)
        );
    }

    #[test]
    fn respects_work_area_origin_for_top_or_left_taskbar() {
        // 上端タスクバー: 作業領域の原点が y=40 にずれる。
        assert_eq!(
            bottom_right_position(area(0, 40, 1920, 1040), 320, 360, 16),
            (1584, 40 + 1040 - 360 - 16)
        );
        // 左端タスクバー: 作業領域の原点が x=60 にずれる。
        assert_eq!(
            bottom_right_position(area(60, 0, 1860, 1080), 320, 360, 16),
            (60 + 1860 - 320 - 16, 704)
        );
    }

    #[test]
    fn handles_negative_origin_on_multi_monitor_layout() {
        assert_eq!(
            bottom_right_position(area(-1920, -200, 1920, 1080), 320, 360, 16),
            (-320 - 16, -200 + 1080 - 360 - 16)
        );
    }

    #[test]
    fn clamps_to_work_area_origin_when_window_is_larger() {
        assert_eq!(
            bottom_right_position(area(100, 50, 300, 200), 320, 360, 16),
            (100, 50)
        );
    }

    #[test]
    fn stays_inside_work_area_bounds() {
        let work_area = area(0, 0, 2560, 1400);
        let (x, y) = bottom_right_position(work_area, 480, 540, 24);
        assert!(x >= 0 && x + 480 <= 2560);
        assert!(y >= 0 && y + 540 <= 1400);
    }

    #[test]
    fn converts_logical_size_with_scale_factor() {
        assert_eq!(to_physical(320.0, 1.0), 320);
        assert_eq!(to_physical(320.0, 1.5), 480);
        assert_eq!(to_physical(16.0, 1.25), 20);
        // 異常な倍率は等倍として扱い、0 サイズや巨大サイズにしない。
        assert_eq!(to_physical(320.0, 0.0), 320);
        assert_eq!(to_physical(320.0, f64::NAN), 320);
    }
}
