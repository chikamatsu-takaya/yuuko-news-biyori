# Geminiローカル実通信確認メモ

- 状態: 暫定メモ
- 確認日: 2026-07-30
- 対象環境: Windowsのローカル開発環境
- 対象ブランチ確認時点: `develop`
- 確認時HEAD: `1f1205387f61e1a942bc1d0b30144f1a020aa909`

> [!IMPORTANT]
> この文書は、2026年7月30日時点のローカル検証結果と、別PCへ開発環境を移行した際の再確認手順を残すための**暫定メモ**である。
> 正式なAPIキー運用方針や正式な使用モデルを定める文書ではなく、今後の担当メンバーへの確認やチーム方針の確定によって変更される可能性がある。
> 要件定義書、設計書、セキュリティ方針より優先されない。
> 上記HEADは当時の確認記録であり、現在または将来の正式な基準SHAではない。
> APIキーの値・断片・長さ・ハッシュは記録しない。

## 1. AI Provider設定の確認結果

今回のローカル環境では、次を確認した。

- 設定画面でAI ProviderをGeminiへ変更して保存した。
- 保存後に設定画面を開き直してもGeminiが保持された。
- Tauri app-data配下の設定JSONでも、`ai.provider = gemini`であることを確認した。
- 設定画面とRustバックエンドは、同じapp-data配下の設定ファイルを参照していた。
- 今回の確認範囲では、設定画面の保存・読み込みとバックエンドのProvider選択は正常に動作していた。

これらは当該環境・確認日時点の結果であり、将来の全環境での動作を保証するものではない。

## 2. APIキーの扱い

- Rust側は環境変数`GEMINI_API_KEY`からGemini APIキーを取得する。
- APIキーはReact、設定JSON、Git管理ファイルへ保存しない。
- 今回はPowerShellセッション内の一時環境変数として設定した。
- APIキーの値・断片・長さ・ハッシュは、本メモを含む文書・ログ・画面へ記録しない。
- PowerShellを閉じると、今回の一時設定は失われる。
- 別PCへ移行した場合は、APIキーを改めて安全に設定する必要がある。

次の運用事項は、2026年7月30日時点では未確定である。

- APIキーの所有者
- APIキーの取得担当
- チーム共通キーか開発者ごとのキーか
- 共有・配布方法
- 課金方針・課金先
- ローテーション・失効時の手順
- 本番環境での秘密情報管理方式

チーム方針が確定するまでは、個人の検証結果を正式な運用仕様として扱わない。

## 3. ニュース機能との関係

現行実装では、次の処理にGeminiを使用していない。

- ニュース取得元の選択
- RSS取得
- 取得記事の保存対象判定
- おすすめスコアの算出と表示順の決定

これらは設定済みニュースソース、RSS・HTML取得処理、重複判定、`RecommendationService`のルールベース判定で処理される。

Geminiを使用する可能性があるのは、取得済み記事に対する次の処理である。

- 要約
- ゆうこの再説明
- ゆうこの一言
- 辞書未命中時の用語解説
- AI Providerの接続確認

したがって、Gemini APIキーがなくてもニュース取得・保存・ルールベース推薦は動作する。ProviderがGeminiでも、APIキー未設定またはGemini生成失敗時は、現在の安全設計に従ってMockProviderへフォールバックする。

## 4. 既定モデルでの実通信確認

確認時点のコード上の既定モデルは`gemini-2.5-flash`である。環境変数`GEMINI_MODEL`が未設定の場合、この既定値が使われる。

今回の環境では、モデル一覧の取得結果について次を確認した。

- `models/gemini-2.5-flash`が含まれていた。
- `generateContent`対応モデルとして表示された。

一方、Rustの実通信テストでは、次の結果となった。

```powershell
cargo test --manifest-path src-tauri/Cargo.toml gemini_live_connection_check_is_ok -- --ignored --nocapture
```

- 結果: `GeminiConnectionOutcome::Internal`
- テスト失敗

```powershell
cargo test --manifest-path src-tauri/Cargo.toml gemini_live_smoke_generates_text -- --ignored --nocapture
```

- 結果: HTTP 404 Not Found
- テスト失敗

この結果から、**今回の環境・確認日時点で、既定モデルを使用した生成要求が失敗したこと**は確認できた。ただし、一般的に`gemini-2.5-flash`が使用不能であるとは断定しない。モデル一覧に存在し、`generateContent`対応と表示される一方で生成要求が404になる詳細理由は未確定である。

APIキー、レスポンス本文、送信プロンプト、URLクエリ、HTTPヘッダーは本メモへ記録していない。

## 5. 一時的なモデル切り替え

原因切り分けのため、今回のPowerShellセッション内だけで次の一時環境変数を設定した。

```powershell
$env:GEMINI_MODEL = "gemini-3.6-flash"
```

この設定について、次の点に注意する。

- 今回のローカル検証だけに使用した一時設定である。
- コード上の既定モデルは変更していない。
- `gemini-3.6-flash`を正式モデルとして採用したわけではない。
- PowerShellを閉じると設定は失われる。
- 別PCでは、必要性と利用可否を確認したうえで改めて設定する必要がある。
- チーム方針が確定するまで、設計書やコードの既定値へ反映しない。
- 将来の再確認時に同モデルが存在・利用可能であることを保証する記録ではない。

## 6. 一時指定モデルで成功した確認

`gemini-3.6-flash`を一時指定した状態で、今回のローカル環境では次を確認できた。

- Gemini APIへの実通信に成功した。
- MockProviderへフォールバックせず、用語解説が表示された。
- 以前表示されていた`(normal / mock)`が表示されなくなった。
- 辞書未登録語の短い説明が表示された。
- 辞書未登録語の詳細説明が表示された。
- DictionaryServiceの厳格JSON解析を通過した。
- 用語を辞書へ保存できた。
- 登録済み辞書一覧へ反映された。
- 保存済み状態を画面で確認できた。

生成された具体的な文章、選択した用語、ニュース本文、Geminiレスポンス本文は記録しない。

この成功結果も、当該PC・APIキー・API側状態・確認日時点での検証結果であり、正式なモデル選定や将来の動作保証を意味しない。

## 7. 別PCでの再確認手順

### 7.1 開発環境の準備

1. リポジトリをセットアップする。
2. `docs/01_setup/開発環境構築手順書.md`に従い、Rust、Node.js、pnpm、Tauriの開発環境を準備する。
3. チームの最新方針を確認したうえで、検証に使用できるGemini APIキーを用意する。
4. APIキーをPowerShellの一時環境変数へ安全に設定する。

APIキーを画面へ表示しない入力例:

```powershell
$secureKey = Read-Host "Gemini API Key" -AsSecureString
$pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secureKey)

try {
    $plainKey = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)

    if ([string]::IsNullOrWhiteSpace($plainKey)) {
        throw "APIキーが入力されていません。"
    }

    $env:GEMINI_API_KEY = $plainKey
}
finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer)
    Remove-Variable plainKey -ErrorAction SilentlyContinue
    Remove-Variable secureKey -ErrorAction SilentlyContinue
}
```

この方法でも、環境変数自体はPowerShellプロセスと子プロセスから参照可能になる。共有PCや信頼できない環境では実行しない。

5. APIキーの値を表示せず、状態だけを確認する。

```powershell
if (-not (Test-Path Env:GEMINI_API_KEY)) {
    "設定なし"
}
elseif ([string]::IsNullOrWhiteSpace($env:GEMINI_API_KEY)) {
    "空値"
}
else {
    "設定あり"
}
```

### 7.2 モデルと実通信の再確認

6. 検証用モデルを、同じPowerShell内で一時指定する。

```powershell
$env:GEMINI_MODEL = "gemini-3.6-flash"
```

この値は2026年7月30日の検証値である。再確認時には、チーム方針、コード上の既定値、Gemini側のモデル提供状況を確認し、利用可能な検証対象か判断する。

7. 接続確認テストを1回実行する。

```powershell
cargo test --manifest-path src-tauri/Cargo.toml gemini_live_connection_check_is_ok -- --ignored --nocapture
```

8. 通常生成テストを1回実行する。

```powershell
cargo test --manifest-path src-tauri/Cargo.toml gemini_live_smoke_generates_text -- --ignored --nocapture
```

接続確認の成功は、APIキー・ネットワーク・モデルへの最小要求が通ることの確認である。用語解説用プロンプト、DictionaryServiceの厳格JSON解析、辞書保存までの成功を単独では保証しない。

### 7.3 アプリでの通し確認

9. テストに使用したものと同じPowerShellからアプリを起動する。

```powershell
pnpm tauri dev
```

10. 設定画面でAI ProviderをGeminiにして保存する。
11. 設定画面を開き直し、Geminiが保持されていることを確認する。
12. 実ニュース記事で辞書未登録語の解説を1回実行する。
13. 解説に`(normal / mock)`などのMock表示が含まれないことを確認する。
14. 短い説明と詳細説明が表示されることを確認する。
15. 用語を辞書へ保存する。
16. 登録済み辞書一覧への反映と保存済み状態を確認する。

失敗した場合も、APIキー、プロンプト、ニュース本文、レスポンス本文をログやIssueへ貼り付けない。Provider、モデルの一時指定有無、固定化された安全なエラー分類、実行した確認段階だけを記録する。

### 7.4 確認終了時の一時環境変数の削除

確認を終えたら、同じPowerShellで一時環境変数を削除する。

```powershell
Remove-Item Env:GEMINI_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:GEMINI_MODEL -ErrorAction SilentlyContinue
```

この操作は現在のPowerShellプロセスに設定した値だけを削除する。別の方法で永続化した環境変数は対象外である。

## 8. 未確定事項と今後の扱い

次の事項は本メモでは決定しない。

- 正式に使用するGeminiモデル
- コード上の既定モデルを変更するか
- 開発者間でのAPIキー共有方法
- 本番環境の秘密情報管理
- 課金・利用上限・予算管理
- APIキーの取得、失効、ローテーション手順
- モデル一覧に存在する既定モデルが404になった詳細原因

これらは担当メンバーと確認し、必要に応じて要件定義書、設計書、セキュリティ方針、運用手順へ正式に反映する。本メモだけを根拠にコードや正式仕様を変更しない。

## 9. 関連資料

- [開発環境構築手順書](./開発環境構築手順書.md)
- [セキュリティ詳細設計書](../02_design/セキュリティ詳細設計書.md)
- [設定領域のai-context](../00_project/ai-context/settings.md)
- [用語解説・ゆうこ辞書領域のai-context](../00_project/ai-context/term-dictionary.md)
- [`gemini_client.rs`](../../src-tauri/src/infra/gemini_client.rs)
- [`ai_provider_service.rs`](../../src-tauri/src/services/ai_provider_service.rs)
