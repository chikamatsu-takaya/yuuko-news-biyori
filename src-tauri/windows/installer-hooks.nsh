; NSIS インストーラーのフック（tauri.conf.json の bundle.windows.nsis.installerHooks）。
;
; アンインストールのとき、初回ダウンロードで取得したローカルLLMのモデルの置き場
; （%LOCALAPPDATA%\<識別子>\local_llm\models。途中のファイル .part と記録 .part.json を含む）を必ず消す。
; 藤井さんの判断（2026-11-18）「モデルだけ残っててもかなり迷惑になりかねない」。
; 設計案（ローカルLLMモデル初回ダウンロード_設計案.md）の Q10。
;
; 消すのはこのフォルダだけ。記事・設定・辞書などほかのアプリのデータ（%APPDATA%\<識別子>・
; %LOCALAPPDATA%\<識別子> の残り）は、これまでどおり「Delete the application data」のチェックを
; 入れたときだけ Tauri のテンプレートが消す。
;
; 消さないとき（モデルを取り直させない）:
;   - 更新（/UPDATE）。Tauri の更新は古い版を消さずに上書きする。
;   - 新しい版のインストーラーが古い版のアンインストーラーを呼んだとき（版を上げるときの既定の選択）。
;     このときインストーラーは `_?=<インストール先>` を付けてアンインストーラーをその場で動かすため、
;     $EXEDIR がインストール先と同じになる。設定 → アプリ や uninstall.exe から始めた通常の
;     アンインストールでは、NSIS がアンインストーラーを一時フォルダへ写して動かすため、$EXEDIR は
;     インストール先と違う。
;
; 広いフォルダを消さないための守り:
;   - 識別子（BUNDLEID）が空ならインストーラーを作らない（コンパイル時に止める）。
;   - $LOCALAPPDATA は "X:\..." の形で、フォルダとして在るときだけ使う（空なら "\<識別子>\..." が
;     ドライブの直下を指してしまうため）。
;   - 消す場所は「$LOCALAPPDATA\<識別子>\local_llm\models」の固定の形だけで、在るときだけ消す。
;     その上の local_llm は空のときだけ消す（中身があれば残る）。

!include LogicLib.nsh

; Tauri のテンプレートの Section Uninstall の終わり（ファイル・ショートカット・レジストリと、
; チェックを入れたときのアプリのデータを消した後）で展開される。
!macro NSIS_HOOK_POSTUNINSTALL
  !if "${BUNDLEID}" == ""
    !error "BUNDLEID is empty: refusing to build the model cleanup hook"
  !endif

  Push $0
  Push $1
  Push $2

  ${If} $UpdateMode = 1
    DetailPrint "Local LLM model: kept (update mode)"
  ${ElseIf} $EXEDIR == $INSTDIR
    DetailPrint "Local LLM model: kept (uninstall called by a newer installer)"
  ${Else}
    ; 利用者ごとの %LOCALAPPDATA%（アプリが app_local_data_dir() として使う場所）を指す。
    SetShellVarContext current
    StrCpy $0 "$LOCALAPPDATA"
    StrLen $1 $0
    StrCpy $2 $0 2 1
    ${If} $1 < 4
    ${OrIf} $2 != ":\"
    ${OrIfNot} ${FileExists} "$0\*.*"
      DetailPrint "Local LLM model: skipped (LOCALAPPDATA is not usable)"
    ${ElseIf} ${FileExists} "$0\${BUNDLEID}\local_llm\models\*.*"
      DetailPrint "Local LLM model: removing $0\${BUNDLEID}\local_llm\models"
      RMDir /r "$0\${BUNDLEID}\local_llm\models"
      RMDir "$0\${BUNDLEID}\local_llm"
    ${EndIf}
  ${EndIf}

  Pop $2
  Pop $1
  Pop $0
!macroend
