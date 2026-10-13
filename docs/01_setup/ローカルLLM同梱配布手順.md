# ローカルLLM同梱配布手順

同梱ローカルLLM（llama.cpp の `llama-server` ＋ Qwen3.5-2B Q4_K_M。判断台帳 D99〜D102）を入れた配布用インストーラーを作る手順と、ライセンス本文の置き場所、実機での速度・メモリの実測結果をまとめる。

- 開発環境で「ローカル」を動かすだけなら `開発環境構築手順書.md` §17.5 で足りる。本書は **配布物を作る人** 向け。
- 設計は `docs/02_design/詳細設計書.md` §5.4.5.1（LocalLlmService）。
- 「未検証」と書いた箇所は、本書の作成時（2026-10-13）にインストーラーを実際には作っていない部分。初めて配布物を作る人が確かめ、結果を本書へ追記する。

---

## 1. 同梱するもの

| 置き場所（`src-tauri/resources/local_llm/` 配下） | 中身 | 大きさ | git |
|---|---|---|---|
| `runtime/` | `llama-server.exe` と、それが使う DLL 21 個（llama.cpp b11269 win-cpu-x64。表は `src-tauri/src/infra/local_llm_runtime.rs` の `RUNTIME_FILES`）＋ Visual C++ ランタイム 3 個（`vcruntime140.dll` / `vcruntime140_1.dll` / `msvcp140.dll`。判断台帳 D112。表は同じファイルの `VC_RUNTIME_FILES`） | 計 44,453,280 B（約 42.4 MiB。うち VC++ ランタイム 732,064 B） | 追跡しない（`.gitignore`） |
| `models/stario-qwen3.5-2b-q4_k_m.gguf` | Qwen3.5-2B を GGUF・Q4_K_M に変換・量子化したもの（`MODEL_FILE` / `MODEL_SIZE` / `MODEL_SHA256`） | 1,312,164,800 B（約 1.22 GiB） | 追跡しない（`.gitignore`） |
| `LICENSES/` | 同梱部品のライセンス本文と `README.txt`（改変の告知・VC++ ランタイムの再頒布の表記を含む） | 約 58 KiB | **追跡する** |

合計は約 1.36 GB（1,356,676,964 B ＝ runtime＋モデル＋LICENSES。2026-10-13 に VC++ ランタイムを加えた後の値）。GGUF はほぼ圧縮が効かないため、インストーラーもおおむねこの大きさになる（未検証）。

### 1.1 Visual C++ ランタイムを同梱する理由と範囲（判断台帳 D112）

- llama.cpp の部品は Visual C++ ランタイムを読み込む（`dumpbin /dependents` で確認。2026-10-13）。`llama-server.exe` は `VCRUNTIME140.dll`、`llama-server-impl.dll` / `llama-common.dll` / `llama.dll` は `MSVCP140.dll` / `VCRUNTIME140.dll` / `VCRUNTIME140_1.dll`、そのほかの `ggml*.dll` / `mtmd.dll` は `MSVCP140.dll` / `VCRUNTIME140.dll`。`libomp.dll` は `KERNEL32` / `PSAPI` だけ。
- VC++ ランタイムの 3 つ自身が読み込むのも、この 3 つと `api-ms-win-crt-*` と `KERNEL32` だけ。`msvcp140_1` / `msvcp140_2` / `vcruntime140_threads` / `concrt140` などは、どの部品も読み込まないため同梱しない。
- `api-ms-win-crt-*`（ユニバーサル C ランタイム・UCRT）は Windows 10 以降に標準で入っているため同梱しない。
- Windows は、実行ファイルと同じフォルダの DLL を先に探す。`runtime/`（`llama-server.exe` の隣）に置くことで、VC++ 再頒布可能パッケージが入っていないパソコンでも起動できる。入っているパソコンでも、`llama-server` は隣に置いた版を使う。
- 入手元: Visual Studio 2022 Build Tools の再頒布用フォルダ `VC/Redist/MSVC/14.44.35112/x64/Microsoft.VC143.CRT/`（ファイルの版 14.44.35211.0）。インターネットからは取らない。版を替えるときは `VC_RUNTIME_FILES` の SHA-256 も直す。

`tauri.conf.json` の `bundle.resources` で `resources/local_llm` → `local_llm` に写され、インストール先のリソースフォルダ（`resource_dir/local_llm`）に入る。アプリは `resolve_bundle_dir` でここを見る。

## 2. 同梱物を置く・確かめる

インターネットからは取らない。手元にある同梱物のフォルダ（`runtime/` と `models/` を持つもの）と、Visual Studio の VC++ ランタイムの再頒布用フォルダ（§1.1）から写す。

```powershell
# リポジトリの一番上で
node scripts/local-llm/place-bundle.mjs <同梱物のフォルダ> --vc-crt <VC++ ランタイムのフォルダ>
# 例（参照アプリ「すたりお」の作業ツリーと Visual Studio 2022 Build Tools がある場合）
node scripts/local-llm/place-bundle.mjs C:/WS/saiyou_creater/src-tauri/resources/local_llm --vc-crt "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Redist/MSVC/14.44.35112/x64/Microsoft.VC143.CRT"

# 置いた後に照合だけをやり直す（インストーラー作成の直前にも自動で走る）
node scripts/local-llm/check-bundle.mjs
```

- `place-bundle.mjs`: Rust の表（`RUNTIME_FILES` / `VC_RUNTIME_FILES` / `MODEL_*`）にあるファイルだけを写す。llama.cpp の部品は `<同梱物のフォルダ>/runtime/` から、VC++ ランタイムは `--vc-crt` のフォルダから写す（`--vc-crt` を省くと `<同梱物のフォルダ>/runtime/` を探し、無ければ指定のしかたを示して止める）。別のプロジェクトの同梱物のフォルダへ VC++ ランタイムを写し足す必要はない。写す前に元を、写した後に置いたものを SHA-256 で照合し、合わなければ止める。`runtime/`・`models/` の中は `.gitignore` 以外を一度消してから写す（前の版や表に無い DLL を残さない）。
- `check-bundle.mjs`: 置いてあるものを同じ表と照合する。`tauri.conf.json` の `build.beforeBundleCommand` に入っているため、`pnpm tauri build` のときはインストーラーを作る直前に必ず走り、欠け・不一致があればバンドルを止める（`tauri dev`・cargo のビルド・テスト・CI では走らない）。
- 置いたファイル（runtime・モデル）は **コミットしない**。`git status` に出ないことを確かめる（`LICENSES/` は追跡対象）。

## 3. 配布用インストーラーを作る

### 3.1 前提

- `開発環境構築手順書.md` の Node.js / pnpm / Rust / C++ Build Tools がそろっていること。
- 初めて `pnpm tauri build` を動かすとき、Tauri CLI はインストーラー作成ツール（WiX Toolset v3・NSIS）を自動でダウンロードする（Tauri v2 の既定の動作）。**ビルドするパソコンにインターネット接続が要る**。社内の取り決めでツールの取得に確認が要る場合は、先に確認する。
- ビルドの出力先（`src-tauri/target/`）に、同梱物の写し（tauri-build が `target/<profile>/local_llm` へ写す約 1.3GB）とインストーラー（約 1.3GB × 形式の数）が増える。空き容量を 5GB 以上見ておく（目安・未検証）。

### 3.2 コマンド

```powershell
# 1. 同梱物を置く（§2）。済んでいれば照合だけ
node scripts/local-llm/check-bundle.mjs

# 2. 配布用ビルド（next build → cargo build --release → check-bundle → インストーラー作成）
pnpm tauri build

# 3. MSI の作成が失敗したときだけ: NSIS だけを作る（判断台帳 D111。未検証）
pnpm tauri build --bundles nsis
```

- `tauri.conf.json` は `bundle.targets: "all"` のまま（判断台帳 D111）。Windows では **MSI（WiX）と NSIS（.exe）の両方** を作る。
- MSI の作成が失敗した（2GB 上限など。§4）ときは、設定を変えずに `pnpm tauri build --bundles nsis` で NSIS だけを作って配る。
- 出力先（Tauri v2 の既定）: `src-tauri/target/release/bundle/nsis/*-setup.exe`、`src-tauri/target/release/bundle/msi/*.msi`。
- ビルド前に、APIキー・`.env`・`app-data/` の実データが混入していないことを確かめる（`開発環境構築手順書.md` §15.3）。

### 3.3 作った後に確かめること（未検証・初回は必ず実施）

1. インストーラーの大きさ（約 1.3GB 前後か）。
2. ネットにつながっていない、または **Visual C++ 再頒布可能パッケージが入っていない** きれいな Windows にインストールし、設定の AI を「ローカル」にして接続テストが「利用できます」になるか（手順は §3.4）。
3. インストール先の `local_llm/LICENSES/` にライセンス本文があるか。
4. アンインストールで `local_llm/` が消えるか。

### 3.4 VC++ ランタイムが入っていないパソコンで確かめる（判断台帳 D112・未検証）

開発用のパソコンには VC++ 再頒布可能パッケージが入っていることが多く、同梱した DLL が無くても動いてしまう。同梱の効果は、入っていない環境で確かめる。

1. 環境を用意する。次のどちらか（どちらも VC++ 再頒布可能パッケージを入れない）。
   - Windows サンドボックス（Windows 10/11 Pro 以上。「Windows の機能」で有効にする）。起動のたびに初期状態に戻り、VC++ ランタイムは入っていない。
   - 新しく入れた Windows の仮想マシン。
2. 入っていないことを確かめる（PowerShell）。次の結果が `False` であること。

   ```powershell
   Test-Path "$env:SystemRoot\System32\vcruntime140_1.dll"
   ```

3. インストーラーを写して入れ、ゆうこを起動する。設定の AI を「ローカル」にして接続テストを押し、「利用できます」になるか。要約などを 1 回生成できるか。
4. 比べる場合: インストール先の `local_llm/runtime/` から `vcruntime140_1.dll` を一時的に別の場所へ移すと、照合で「見つからない」（`LocalAiFailure::Missing`）の固定の文言になり、`llama-server` を起動しないことを確かめられる（確かめたら元へ戻す）。
5. 結果（日付・Windows の版・環境・成否）を本書へ追記する。

インストーラーを作らずに確かめたい場合は、`src-tauri/resources/local_llm/runtime/` を丸ごと上の環境へ写し、PowerShell で `.\llama-server.exe --version` が版を表示して終わるかを見る（DLL が足りなければ「コンピューターに ～.dll がないため」の画面が出る）。

## 4. MSI の 2GB 制限と配布形式（判断台帳 D111）

### 4.1 事実関係

- Windows Installer（MSI）と WiX には、1 ファイル・1 キャビネット（CAB）・MSI ファイル自体についておおむね **2GB の上限** がある。Tauri の WiX テンプレートは同梱物を 1 つの CAB にまとめて MSI へ埋め込む（`EmbedCab`）ため、**同梱物の合計が 2GB に近づくと MSI が作れない・壊れる** おそれがある（Tauri の公開資料・既知の不具合報告からの知識。本プロジェクトでは未検証）。
- 現在の同梱物は約 1.36GB（§1）で、2GB には収まる見込み。ただし余裕は 0.6GB ほどしかなく、**モデルを大きくする（例: 4B 級の Q4 は約 2.5GB）と MSI は確実に上限を超える**。
- NSIS にも、インストーラーに詰めるデータ全体でおおむね 2GB の上限がある（32 ビットのオフセット）。現在の大きさなら NSIS でも収まる見込み（未検証）。2GB を超えるモデルを同梱するなら、どちらの形式でも同梱をやめて「初回ダウンロード」（`resolve_bundle_dir` の差し替え・D101 の将来案）を検討する。
- WiX は 1.3GB の CAB を高圧縮で作るため、ビルドに時間がかかる（GGUF はほぼ縮まない）。NSIS は `bundle.windows.nsis.compression` で圧縮方式（`lzma` / `zlib` / `bzip2` / `none`）を選べる（未検証）。

### 4.2 決定した運用（判断台帳 D111・2026-10-13）

- `bundle.targets` は `"all"` のまま、**MSI と NSIS の両方を作る**。
- **MSI の作成が失敗したら、`pnpm tauri build --bundles nsis` で NSIS だけを作る**（設定は変えない）。
- 配るのは作れた形式。両方作れた場合の使い分けは配布先の都合に合わせる。

背景（NSIS を代わりの形式にする理由）:

- 1 つの .exe で配れる。Tauri の NSIS は既定で利用者ごと（管理者権限なし）にインストールできる。
- MSI の CAB / MSI ファイルの 2GB 上限と、大きな CAB の作成時間を避けられる。
- 同梱物が増えて MSI が先に作れなくなっても、NSIS は約 2GB まで同じ同梱物で作れる見込み（未検証）。

- NSIS の圧縮は、GGUF がほぼ縮まないため `"bundle": { "windows": { "nsis": { "compression": "zlib" } } }` などへ下げるとビルドが速くなる可能性がある（大きさへの影響は小さい見込み。未検証）。

## 5. ライセンス表記

- 置き場所: `src-tauri/resources/local_llm/LICENSES/`（git で追跡）。`bundle.resources` で `local_llm/LICENSES/` としてインストーラーへ同梱される。
- `README.txt` に、各部品の入手元・版・ライセンス本文のファイル名と、Qwen の重みを GGUF へ変換・量子化した **改変の告知**（Apache License 2.0 第4条(b)）を書いている。文言は持ち主の確認対象のため、変更するときは持ち主に確認する。
- 同梱部品とライセンス本文の対応（2026-10-13 に `RUNTIME_FILES` と実物の DLL を突き合わせて確認）:

| 同梱物 | ライセンス本文 |
|---|---|
| `models/stario-qwen3.5-2b-q4_k_m.gguf`（Qwen3.5-2B） | `Qwen3.5-Apache-2.0.txt`（改変の告知は `README.txt`） |
| `llama-server.exe` / `llama-server-impl.dll` / `llama-common.dll` / `llama.dll` / `mtmd.dll` / `ggml.dll` / `ggml-base.dll` / `ggml-cpu-*.dll`（CPU の種類ごとの 14 個） | `llama.cpp-MIT.txt`（ggml を含む） |
| `libomp.dll` | `LLVM-OpenMP-Apache-2.0-with-LLVM-exception.txt` |
| `vcruntime140.dll` / `vcruntime140_1.dll` / `msvcp140.dll`（Microsoft Visual C++ ランタイム。D112） | 本文ファイルは置かない。`README.txt` §3 に「Visual Studio のライセンス条項（再頒布可能コードの条件）に従って再頒布する」旨を書いている（VS の再頒布用フォルダにライセンス本文のファイルは無い。2026-10-13 確認） |
| 実行ファイルへ組み込まれた部品: cpp-httplib / nlohmann-json / BoringSSL / stb_image / miniaudio / subprocess.h | 各 `*.txt`（`README.txt` §2 に一覧） |

- 実物の DLL の中の文字列からも、httplib・nlohmann・BoringSSL・subprocess の組み込みを確認した（llguidance は「有効でない」旨の文言だけで、組み込まれていないと見られる）。
- アプリ本体（Rust クレート・npm パッケージ）の第三者ライセンス表記は本書の対象外。

## 6. 実機での速度・メモリの実測

### 6.1 結果（2026-10-13・静かな状態で再計測。判断台帳 D113）

**計測した機械**: Intel Core Ultra 5 125U（論理 14 コア）・メモリ 15.5GB・Windows 11 Pro 26200。llama-server のスレッドは `-t 4`（論理コア数の半分・上限 4）、`-c 4096`、CPU のみ。VC++ ランタイムは同梱した版（§1.1）を使った。

**計測時の状態**: ほかの開発作業（並列ビルド・別のテスト）は動かしていない。ただし持ち主のアプリ（ブラウザ・チャットなど）は開いたままで、**完全な無負荷ではない**。各回の直前の空きメモリ（`\Memory\Available MBytes`）は 2.9〜4.9GB。生成中は持ち主のアプリの動きで空きメモリが上下し、記事の回では一時的に 0.1〜0.6GB まで下がった（下表）。テストは debug ビルドで各 3 回、照合の時間を見るため release ビルドで各 1 回動かした。

| 項目 | 1回目 | 2回目 | 3回目 | release（1回） | メモ |
|---|---|---|---|---|---|
| 直前の空きメモリ（記事のテスト） | 3,394 MB | 3,982 MB | 2,916 MB | 4,932 MB | 生成中の最小: 144 / 649 / 518 / —MB |
| 照合（モデル・部品の SHA-256） | 約 33 秒 | 29.7 秒 | 31.0 秒 | **約 1〜2 秒** | debug はテストのプロセスが現れてから llama-server が現れるまで（0.5 秒刻み。1回目はプロセスの開始時刻を取り損ねたため、テストの所要時間からの推定）。release は接続テスト全体が 3.6 秒（用語解説の回では llama-server が現れるまで約 1.3 秒）。続けて動かしたため、モデルは OS のキャッシュに載っていた |
| 接続テスト（照合＋起動＋16トークン生成） | 35.0 秒 | 32.9 秒 | 33.7 秒 | 3.6 秒 | 照合を除く起動＋生成は 3 秒前後 |
| 要約 `summary_v1`（入力 848 文字） | 13.1 秒 | 12.3 秒 | 10.1 秒 | 9.3 秒 | 出力 65〜114 文字 |
| 要点＋タグ `article_points_v2` | 21.5 秒 | 25.3 秒 | 20.0 秒 | 17.2 秒 | 出力 310〜326 文字。1回の生成で最も長い |
| 再説明 `yuuko_explanation_v1` | 11.9 秒 | 13.0 秒 | 11.4 秒 | 10.3 秒 | |
| 一言 `yuuko_comment_v1` | 8.2 秒 | 7.9 秒 | 9.5 秒 | 7.5 秒 | |
| 記事1件の4生成の合計 | 54.7 秒 | 58.5 秒 | 51.0 秒 | 44.3 秒 | 自動要約1件ぶん（起動済みのとき） |
| 直前の空きメモリ（用語解説のテスト） | 4,277 MB | 4,865 MB | 4,855 MB | 4,773 MB | 生成中の最小: 2,576 / 2,645 / 2,990 / —MB |
| 用語解説 1語目（補助金・Simple。照合＋起動を含む） | 34.5 秒 | 29.8 秒 | 32.1 秒 | 9.6 秒 | 照合を除けば 10 秒前後 |
| 用語解説 クラウドサービス（Normal） | 9.6 秒 | 11.2 秒 | 10.7 秒 | 11.3 秒 | |
| 用語解説 デジタル化（Detailed） | 16.2 秒 | 13.3 秒 | 14.7 秒 | 11.5 秒 | |
| 用語解説 `Vec<String>`（Normal） | 11.4 秒 | 10.4 秒 | 10.3 秒 | 11.2 秒 | |

- 8 回（debug 6・release 2）とも失敗・打ち切りなし。1回の生成の最長は 25.3 秒（要点＋タグ）で、上限 180 秒の約 1/7。
- release の生成がやや速いのは、llama-server は同じ実行ファイルのため、直前の重い照合（debug の SHA-256 で CPU を約 30 秒使う）による発熱・クロック低下の差と見ている（未検証）。
- 生成中の llama-server の CPU 時間は、経過 1 秒あたり約 3.9 秒（4 スレッドをほぼ使い切る）。

| メモリ（llama-server） | ワーキングセット | プライベート | メモ |
|---|---|---|---|
| 生成中のピーク（記事・3回） | 2,272〜2,274 MB | 1,481〜1,483 MB | 生成が進むほど少しずつ増える（文脈のキャッシュ） |
| 生成中のピーク（用語解説・3回） | 2,137〜2,138 MB | 1,352〜1,354 MB | |
| 起動直後（モデル読み込み後） | 約 2,020〜2,090 MB | 約 1,240〜1,300 MB | 待機だけの時間は今回測っていない（テストは起動後すぐ生成する）。前回の待機値は下の暫定表 |

- 前回（暫定値）と比べ、メモリの値はほぼ同じ。前回 OS にワーキングセットを削られた回（1,328 MB まで減少）のような落ち込みは今回は無かった。
- アプリ本体（Tauri＋WebView2）のメモリは **計測していない**（テストは Rust の処理だけを動かすため）。参考の常駐時の値は `軽量常駐性能確認手順.md` を見る。
- ローカル AI を使っている間は、上の約 2.1〜2.3GB がアプリ本体に上乗せされる。最後に使ってから5分で止まり、そのメモリは戻る（詳細設計書 §5.4.5.1）。

#### 6.1.1 前回の暫定値（2026-10-13・比較用。メモリに余裕のない状態）

> 下の数値はほかの作業（並列ビルドなど）を動かしたまま、debug ビルドで測ったもの。上の再計測と比べるために残す。

**計測した機械**: Intel Core Ultra 5 125U（論理 14 コア）・メモリ 15.5GB・Windows 11 Pro 26200・AC 電源（電源プラン「HP Optimized」）。llama-server のスレッドは `-t 4`（論理コア数の半分・上限 4）、`-c 4096`、CPU のみ。

**計測時の状態（重要）**: ほかのアプリ（ブラウザ・Discord・Slack・Teams・セキュリティ製品の仮想環境など）を開いたままで、**空きメモリは 1.1〜2.0GB、ページング（Pages/sec）は 1.3〜2.0 万/秒** と、メモリに余裕のない状態だった。数値はこの条件のもの。

| 項目 | 1回目 | 2回目 | 3回目 | メモ |
|---|---|---|---|---|
| 照合（モデル・部品の SHA-256。アプリ起動中、照合が通るまで。一度通れば `verified` を覚え、5分使わずに止まった後の再起動でもやり直さない） | 約 39 秒 | 約 28 秒 | 約 33 秒 | テスト（debug ビルド）で、テスト開始から llama-server が現れるまでの時間から推定。最適化した SHA-256（`Get-FileHash`）では 1.22GB のモデルが 2.8〜3.6 秒のため、配布版（release）は数秒の見込み（未検証） |
| 起動（照合なし・`/health` が応えるまで） | 4.0 秒 | 4.2 秒 | — | llama-server を同じ引数で直接起動して計測 |
| 接続テスト（照合＋起動＋16トークン生成） | 120.3 秒 | 32.4 秒 | 38.5 秒 | 1回目はファイルが OS のキャッシュに無く、ページングも重かった |
| 要約 `summary_v1`（入力 848 文字） | **180 秒で打ち切り（失敗）** | 20.2 秒 | 17.1 秒 | 出力 100〜112 文字 |
| 要点＋タグ `article_points_v2` | — | 27.2 秒 | 45.1 秒 | 出力 303〜317 文字 |
| 再説明 `yuuko_explanation_v1` | — | 13.5 秒 | 12.7 秒 | |
| 一言 `yuuko_comment_v1` | — | 10.3 秒 | 13.6 秒 | |
| 記事1件の4生成の合計 | — | 71.2 秒 | 88.5 秒 | 自動要約1件ぶん（起動済みのとき） |
| 用語解説 1語目（補助金・Simple。照合＋起動を含む） | 46.1 秒 | 49.5 秒 | 52.4 秒 | 照合・起動を除く生成だけでは 15 秒前後 |
| 用語解説 クラウドサービス（Normal） | 13.6 秒 | **180 秒で打ち切り（失敗）** | 16.6 秒 | |
| 用語解説 デジタル化（Detailed） | 19.3 秒 | — | 24.0 秒 | |
| 用語解説 `Vec<String>`（Normal） | 11.5 秒 | — | 19.7 秒 | |

| メモリ（llama-server） | ワーキングセット | プライベート | メモ |
|---|---|---|---|
| 待機（モデル読み込み後・生成していない） | 約 1,974 MB | 約 1,230 MB | 2回とも同じ。30 秒間ほぼ変化なし。ワーキングセットにはモデルのファイル（メモリへ写した読み取り専用の分）が入る |
| 生成中のピーク | 2,088〜2,273 MB | 1,307〜1,482 MB | 要約 3 回・用語解説 3 回の最大値の範囲。生成が進むほど少しずつ増える（文脈のキャッシュ） |
| メモリが足りないとき | 1,328 MB まで減少 | 約 1,306 MB | OS がワーキングセットを削り、生成が 180 秒で打ち切られた回（用語解説2回目） |

- テストのプロセス（Rust 側の処理）は 27 MB 以下だった。

### 6.2 分かったこと・懸念

- 静かな状態（§6.1）では、起動済みなら要約は 9〜13 秒、用語解説は 10〜16 秒、記事 1 件の自動要約（4 生成）は 44〜59 秒。前回の暫定値（§6.1.1: 要約 17〜20 秒・記事 1 件 71〜89 秒）より 3〜4 割短い。
- 静かな状態では 8 回とも 180 秒の上限に近づかなかった（1回の生成の最長 25.3 秒）。**生成の上限 180 秒と `-t 4`（論理コア数の半分・上限 4）は変えない**（判断台帳 D113 の再計測の結論。変える場合は持ち主が判断する）。理由:
  - 180 秒は最長の 1 回の約 7 倍で、遅いパソコンや発熱でクロックが下がったときの余裕になる。縮めても、メモリ不足でページング待ちになった回（§6.1.1）が早く失敗するだけで、成功する回は増えない。延ばしても、ページング待ちの回が成功する見込みは低く、利用者を待たせる時間が延びる。
  - `-t` は、今回の計測では 4 スレッドをほぼ使い切って上の速さが出ている。増やした場合の比較（`-t 6` など）は、テストからスレッド数を変える口が無いため測っていない（本番コードを実験のためだけに変えない）。常駐アプリとしてほかのアプリの操作感を守るため、根拠なく増やさない。
- **メモリに余裕のない 16GB 機では、生成が 180 秒の上限で打ち切られることがある**（前回の暫定値では 6 回中 2 回）。そのときは llama-server のワーキングセットが OS に削られ、CPU をほとんど使えなくなっていた（ページング待ち）。上限時間やスレッド数ではなくメモリ不足が原因のため、この場合はメモリに余裕ができてからやり直す。
- 照合は debug ビルドでは約 30 秒かかるが、配布版（release）では約 1〜2 秒（モデルが OS のキャッシュに載った状態。キャッシュに無い初回は、ディスクから 1.22GB を読む時間が加わる。未計測）。
- **`llama-server` と DLL は Visual C++ ランタイム（`VCRUNTIME140.dll` / `VCRUNTIME140_1.dll` / `MSVCP140.dll`）を必要とするが、同梱していない。** 再頒布可能パッケージが入っていないパソコンでは起動できない（アプリは「起動できません」系の失敗になる）見込み（未検証）。**決定（判断台帳 D112・2026-10-13）**: `VCRUNTIME140.dll` / `VCRUNTIME140_1.dll` / `MSVCP140.dll` を `runtime/`（`llama-server.exe` と同じ場所）へ同梱し、照合の表にも加える。**対応済み（2026-10-13）**: 照合の表 `VC_RUNTIME_FILES`・`place-bundle.mjs --vc-crt`・§1 の大きさとファイル数・§1.1 の範囲・§5 のライセンス表・`LICENSES/README.txt` §3 に反映した。§6.1.1 の暫定値は同梱前の値（VC++ 再頒布可能パッケージが入ったパソコン）、§6.1 の再計測は同梱した版で測った。入っていないパソコンでの確認（§3.4）は未実施。

### 6.3 計測方法（再計測するとき）

```powershell
# 同梱物を置いたうえで（§2）。共有の target を使う場合は CARGO_TARGET_DIR を指定する
$env:CARGO_TARGET_DIR = "<target の場所>"
# 要約・要点・再説明・一言（接続テストの時間も出る）
cargo test --manifest-path src-tauri/Cargo.toml --lib real_local_llm_generates_article_texts -- --ignored --nocapture
# 用語解説（4語。1語目は照合・起動を含む）
cargo test --manifest-path src-tauri/Cargo.toml --lib real_local_llm_term_explanation_is_strict_json -- --ignored --nocapture
```

- 時間はテストが表示する値（`Instant` で測った1回ごとの所要時間）。
- 照合の時間は debug ビルドと配布版で大きく違う（§6.1）。配布版に近い値は `cargo test --release ...`（同じ引数に `--release` を足す）で見る。debug では、テストのプロセスが現れてから llama-server が現れるまでの時間を照合の時間とした。
- テストのビルドが計測に重ならないよう、先に `cargo test ... --no-run` でビルドしておく。
- メモリは、テストを動かしている間、別の PowerShell で 0.5 秒ごとに `Get-Process llama-server` の `WorkingSet64` / `PrivateMemorySize64` を記録し、最大値を取った。
- 待機時のメモリと起動時間は、`llama-server.exe` をアプリと同じ引数（`-m <モデル> --host 127.0.0.1 --port <空き番号> -c 4096 -np 1 -t 4 --api-key <使い捨ての値> --no-slots --offline`）で直接起動し、`/health` が 200 を返すまでの時間と、その後 30 秒間のメモリを記録した。
- 同じ条件で比べられるよう、空きメモリ（`Get-Counter '\Memory\Available MBytes'`）も一緒に記録する。
