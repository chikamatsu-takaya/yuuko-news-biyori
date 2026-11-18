; NSIS インストーラーのフック（tauri.conf.json の bundle.windows.nsis.installerHooks）。
;
; アンインストールのとき、初回ダウンロードで取得したローカルLLMのモデルの置き場
; （%LOCALAPPDATA%\<識別子>\local_llm\models。途中のファイル .part と記録 .part.json を含む）を必ず消す。
; 藤井さんの判断（2026-11-18）「モデルだけ残っててもかなり迷惑になりかねない」。
; 設計案（ローカルLLMモデル初回ダウンロード_設計案.md）の Q10・§0.3.2、配布手順書 §3.3.2。
;
; 消すのはこのフォルダだけ。記事・設定・辞書などほかのアプリのデータ（%APPDATA%\<識別子>・
; %LOCALAPPDATA%\<識別子> の残り）は、これまでどおり「Delete the application data」のチェックを
; 入れたときだけ Tauri のテンプレートが消す。
;
; 前提: installMode は currentUser（Tauri の既定・本アプリの設定）。perMachine / both にするなら、
; アンインストールする人と %LOCALAPPDATA% の関係が変わるため、本フックを見直す。
;
; 消さないとき（モデルを取り直させない）:
;   - 更新（/UPDATE）。Tauri の更新は古い版を消さずに上書きする。
;   - アンインストーラーがインストール先から直接動いているとき（$EXEDIR == $INSTDIR）。
;     新しい版のインストーラーが「Uninstall before installing」で古い版のアンインストーラーを呼ぶとき
;     （`_?=<インストール先>` を付けるため一時フォルダへ写されない）がこれに当たる。
;     スクリプトなどが `uninstall.exe /S _?=<dir>` で呼んだときも同じ扱いになり、モデルは残る。
;     設定 → アプリ や uninstall.exe のダブルクリックでは、NSIS がアンインストーラーを一時フォルダへ
;     写して動かす（NSIS の仕様）ため、$EXEDIR はインストール先と違い、消す。
;
; 広いフォルダや別の場所を消さないための守り:
;   - 識別子（BUNDLEID）が空ならインストーラーを作らない（コンパイル時に止める）。
;   - $LOCALAPPDATA は "X:\..." の形で、フォルダとして在るときだけ使う（空なら "\<識別子>\..." が
;     ドライブの直下を指してしまうため）。UNC（\server\...）に向け直した環境では消さない（安全側）。
;   - 消す場所は「$LOCALAPPDATA\<識別子>\local_llm\models」の固定の形だけ。外からの入力は使わない。
;   - <識別子>・local_llm・models のどれかが再解析ポイント（ジャンクション・シンボリックリンク）なら
;     何も消さない。RMDir /r はリンクの先までたどって消すため使わない。
;   - models の中は 1 段だけ自分でたどる（取得の置き方は models\<版>\<ファイル> の 1 段だけ。
;     model_download.rs）。リンクの子フォルダはリンクそのものだけを外し（再帰しない RMDir）、
;     普通の子フォルダはその直下のファイルだけを消してから空なら外す。2 段目より下は消さない。
;   - local_llm は空のときだけ外す（中身があれば残る）。

!include LogicLib.nsh

!define YUUKO_FILE_ATTRIBUTE_DIRECTORY 0x10
!define YUUKO_FILE_ATTRIBUTE_REPARSE_POINT 0x400

; _PATH の属性を見て、無い・読めない・再解析ポイントのときに _FLAG を 1 にする（変えるのは $9 だけ）。
!macro YUUKO_MARK_IF_UNSAFE _PATH _FLAG
  System::Call 'kernel32::GetFileAttributesW(w "${_PATH}") i .r9'
  ${If} $9 = -1
    StrCpy ${_FLAG} 1
  ${Else}
    IntOp $9 $9 & ${YUUKO_FILE_ATTRIBUTE_REPARSE_POINT}
    ${If} $9 <> 0
      StrCpy ${_FLAG} 1
    ${EndIf}
  ${EndIf}
!macroend

; _ROOT（%LOCALAPPDATA% の値）の下の <識別子>\local_llm\models を消す。
; 確かめ用の小さな NSIS からも呼べるよう、場所を引数にしている（ラベルを使わず、何度でも展開できる）。
; 使うレジスタは $0〜$6 と $9 で、呼ぶ側で退避する。
!macro YUUKO_REMOVE_DOWNLOADED_MODEL _ROOT _ID
  StrCpy $0 "${_ROOT}"
  StrLen $1 $0
  StrCpy $2 $0 2 1
  ${If} $1 < 4
  ${OrIf} $2 != ":\"
  ${OrIfNot} ${FileExists} "$0\*.*"
    DetailPrint "Local LLM model: skipped (LOCALAPPDATA is not usable)"
  ${ElseIfNot} ${FileExists} "$0\${_ID}\local_llm\models\*.*"
    ; 取得していない（または消してある）。何もしない。
  ${Else}
    StrCpy $1 0
    !insertmacro YUUKO_MARK_IF_UNSAFE "$0\${_ID}" $1
    !insertmacro YUUKO_MARK_IF_UNSAFE "$0\${_ID}\local_llm" $1
    !insertmacro YUUKO_MARK_IF_UNSAFE "$0\${_ID}\local_llm\models" $1
    ${If} $1 = 1
      DetailPrint "Local LLM model: kept (the folder is a link or not readable): $0\${_ID}\local_llm\models"
    ${Else}
      StrCpy $3 "$0\${_ID}\local_llm\models"
      DetailPrint "Local LLM model: removing $3"
      ClearErrors
      FindFirst $4 $5 "$3\*.*"
      ${DoWhile} $5 != ""
        ${If} $5 != "."
        ${AndIf} $5 != ".."
          System::Call 'kernel32::GetFileAttributesW(w "$3\$5") i .r2'
          ${If} $2 <> -1
            IntOp $6 $2 & ${YUUKO_FILE_ATTRIBUTE_DIRECTORY}
            IntOp $2 $2 & ${YUUKO_FILE_ATTRIBUTE_REPARSE_POINT}
            ${If} $2 <> 0
              ; リンクはリンクそのものだけを外す（先は消さない）。
              ${If} $6 <> 0
                RMDir "$3\$5"
              ${Else}
                Delete "$3\$5"
              ${EndIf}
            ${ElseIf} $6 <> 0
              ; 版のフォルダ: 直下のファイルだけを消し、空なら外す（中のフォルダ・リンクはたどらない）。
              Delete "$3\$5\*.*"
              RMDir "$3\$5"
            ${Else}
              Delete "$3\$5"
            ${EndIf}
          ${EndIf}
        ${EndIf}
        ClearErrors
        FindNext $4 $5
      ${Loop}
      FindClose $4
      RMDir "$3"
      ${If} ${FileExists} "$3\*.*"
        ; 動いている llama-server やウイルス対策ソフトがファイルを開いている、など。
        DetailPrint "Local LLM model: could not remove completely: $3"
      ${Else}
        RMDir "$0\${_ID}\local_llm"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

; Tauri のテンプレートの Section Uninstall の終わり（ファイル・ショートカット・レジストリと、
; チェックを入れたときのアプリのデータを消した後）で展開される。
!macro NSIS_HOOK_POSTUNINSTALL
  !if "${BUNDLEID}" == ""
    !error "BUNDLEID is empty: refusing to build the model cleanup hook"
  !endif
  !if "${INSTALLMODE}" != "currentUser"
    !warning "installer-hooks.nsh assumes installMode currentUser: review the model cleanup hook"
  !endif

  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $9

  ${If} $UpdateMode = 1
    DetailPrint "Local LLM model: kept (update mode)"
  ${ElseIf} $EXEDIR == $INSTDIR
    DetailPrint "Local LLM model: kept (uninstaller runs in place, e.g. called by a newer installer)"
  ${Else}
    ; 利用者ごとの %LOCALAPPDATA%（アプリが app_local_data_dir() として使う場所）を指す。
    SetShellVarContext current
    !insertmacro YUUKO_REMOVE_DOWNLOADED_MODEL "$LOCALAPPDATA" "${BUNDLEID}"
  ${EndIf}

  Pop $9
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend
