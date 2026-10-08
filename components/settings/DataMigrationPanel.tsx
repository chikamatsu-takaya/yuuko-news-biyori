"use client";

/**
 * 設定画面「データ管理」タブのデータ移行パネル（画面詳細設計書 SCR-003 §7 / データ設計書 §15.6・§15.7）。
 *
 * - 書き出し: export_migration_data を呼び、結果（ファイル名・件数）を表示する。書き出し先フォルダを開くボタンを出す。
 * - 取り込み: imports/ に置かれたZIPの一覧から選び、「全部置き換わる」確認ダイアログを経てから実行する。
 *   取り込み中は画面全体を覆うダイアログで他の設定操作を止める（settings.json には書き込みロックが無いため）。
 *   取り込み後は再起動を勧める（restartRequired）。「あとで」を選んでも画面は読み込み直して新しいデータを表示する。
 *
 * ZIPの作成・検証・バックアップ・置き換え・フォルダの特定はすべて Rust 側。ここはファイル名と結果だけを扱い、
 * パス・Rust のエラー文は表示しない（失敗は lib/migration-display.mjs の固定文言）。
 */

import * as React from "react";
import {
  Download,
  FolderOpen,
  RefreshCw,
  RotateCcw,
  Upload,
} from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  exportMigrationData,
  importMigrationData,
  listMigrationImports,
  openMigrationFolder,
  restartApp,
  type MigrationExportResult,
  type MigrationFolderKind,
  type MigrationImportCandidate,
  type MigrationImportResult,
} from "@/lib/tauri/migration";
import {
  formatMigrationBytes,
  formatMigrationCreatedAt,
  migrationErrorMessage,
} from "@/lib/migration-display.mjs";

/** パネル内で同時に1つだけ走らせる操作。 */
type MigrationTask = "export" | "list" | "import" | "openFolder" | "restart";

/** 書き出し・取り込みの結果表示（成功・失敗・プレビューでの未実行）。 */
type Outcome =
  | { kind: "success"; text: string }
  | { kind: "error"; text: string }
  | { kind: "info"; text: string };

const PREVIEW_NOTICE =
  "プレビュー表示では実行できないよ。アプリで開いたときに使ってね。";

const consoleErrorKind = (error: unknown) =>
  error instanceof Error ? error.name : typeof error;

export function DataMigrationPanel({
  onImportingChange,
}: {
  /** 取り込みの開始・終了を親へ伝える（親は保存などの操作を止める）。 */
  onImportingChange?: (importing: boolean) => void;
}) {
  const [runningTask, setRunningTask] = React.useState<MigrationTask | null>(
    null
  );
  const [exportResult, setExportResult] =
    React.useState<MigrationExportResult | null>(null);
  const [exportOutcome, setExportOutcome] = React.useState<Outcome | null>(
    null
  );
  const [candidates, setCandidates] = React.useState<
    MigrationImportCandidate[]
  >([]);
  const [listLoaded, setListLoaded] = React.useState(false);
  const [listError, setListError] = React.useState<string | null>(null);
  const [folderError, setFolderError] = React.useState<string | null>(null);
  const [importOutcome, setImportOutcome] = React.useState<Outcome | null>(
    null
  );
  const [confirmTarget, setConfirmTarget] =
    React.useState<MigrationImportCandidate | null>(null);
  const [importResult, setImportResult] =
    React.useState<MigrationImportResult | null>(null);
  const [restartError, setRestartError] = React.useState<string | null>(null);

  // state 更新前の連打でも二重実行しないよう、同期的に参照できる ref でも実行中を保持する。
  const runningRef = React.useRef<MigrationTask | null>(null);
  const isMountedRef = React.useRef(true);
  const onImportingChangeRef = React.useRef(onImportingChange);
  React.useEffect(() => {
    onImportingChangeRef.current = onImportingChange;
  }, [onImportingChange]);

  const begin = (task: MigrationTask) => {
    if (runningRef.current) {
      return false;
    }
    runningRef.current = task;
    setRunningTask(task);
    return true;
  };

  const finish = () => {
    runningRef.current = null;
    if (isMountedRef.current) {
      setRunningTask(null);
    }
  };

  const loadCandidates = React.useCallback(async () => {
    if (runningRef.current) {
      return;
    }
    runningRef.current = "list";
    setRunningTask("list");
    setListError(null);
    try {
      const list = await listMigrationImports();
      if (!isMountedRef.current) {
        return;
      }
      setCandidates(list);
      setListLoaded(true);
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.error(
        "Failed to list migration imports via tauri command:",
        consoleErrorKind(error)
      );
      setListError(migrationErrorMessage("list", error));
    } finally {
      runningRef.current = null;
      if (isMountedRef.current) {
        setRunningTask(null);
      }
    }
  }, []);

  React.useEffect(() => {
    isMountedRef.current = true;
    void loadCandidates();
    return () => {
      isMountedRef.current = false;
    };
  }, [loadCandidates]);

  const handleExport = async () => {
    if (!begin("export")) {
      return;
    }
    setExportOutcome(null);
    setExportResult(null);
    try {
      const result = await exportMigrationData();
      if (!isMountedRef.current) {
        return;
      }
      if (!result) {
        setExportOutcome({ kind: "info", text: PREVIEW_NOTICE });
        return;
      }
      setExportResult(result);
      setExportOutcome({ kind: "success", text: "書き出しが終わったよ！" });
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.error(
        "Failed to export migration data via tauri command:",
        consoleErrorKind(error)
      );
      setExportOutcome({
        kind: "error",
        text: migrationErrorMessage("export", error),
      });
    } finally {
      finish();
    }
  };

  const handleOpenFolder = async (kind: MigrationFolderKind) => {
    if (!begin("openFolder")) {
      return;
    }
    setFolderError(null);
    try {
      const opened = await openMigrationFolder(kind);
      if (!opened && isMountedRef.current) {
        setFolderError(PREVIEW_NOTICE);
      }
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.error(
        "Failed to open migration folder via tauri command:",
        consoleErrorKind(error)
      );
      setFolderError(migrationErrorMessage("openFolder", error));
    } finally {
      finish();
    }
  };

  // 確認ダイアログで「取り込む」を押した後だけ呼ばれる。
  const handleConfirmImport = async () => {
    const target = confirmTarget;
    setConfirmTarget(null);
    if (!target || !begin("import")) {
      return;
    }
    setImportOutcome(null);
    onImportingChangeRef.current?.(true);
    try {
      const result = await importMigrationData(target.fileName);
      if (!isMountedRef.current) {
        return;
      }
      if (!result) {
        setImportOutcome({ kind: "info", text: PREVIEW_NOTICE });
        return;
      }
      setRestartError(null);
      setImportResult(result);
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.error(
        "Failed to import migration data via tauri command:",
        consoleErrorKind(error)
      );
      setImportOutcome({
        kind: "error",
        text: migrationErrorMessage("import", error),
      });
    } finally {
      onImportingChangeRef.current?.(false);
      finish();
    }
  };

  // 取り込み後の再起動（restartRequired）。Rust の AppHandle::request_restart で
  // 常駐処理（自動要約キュー・報酬同期など）も新しいデータで始め直す。
  const handleRestart = async () => {
    if (!begin("restart")) {
      return;
    }
    setRestartError(null);
    try {
      const restarted = await restartApp();
      if (!restarted) {
        // 非Tauri（プレビュー）では再起動できないため、画面だけ読み込み直す。
        window.location.reload();
      }
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.error(
        "Failed to restart the app via tauri command:",
        consoleErrorKind(error)
      );
      setRestartError(migrationErrorMessage("restart", error));
    } finally {
      finish();
    }
  };

  // 「あとで」でも、表示中の値は取り込み前のものなので、画面を読み込み直して新しいデータを表示する。
  const handleReloadLater = () => {
    window.location.reload();
  };

  const isBusy = runningTask !== null;
  const isImporting = runningTask === "import";

  return (
    <>
      <Card className="border-0 shadow-sm" data-testid="migration-export-card">
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-base text-[var(--yuuko-green)]">
            <Download className="w-5 h-5" />
            データの書き出し
          </CardTitle>
        </CardHeader>
        <CardContent className="pt-0 space-y-3">
          <p className="text-xs text-muted-foreground leading-relaxed">
            PCを変えるときのために、設定・ニュース・お気に入り・辞書・友情ランク・報酬・ガチャ・アーカイブを1つのZIPにまとめるよ。APIキーやニュース取得元の設定は入らないよ。
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              className="bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white gap-2"
              onClick={() => void handleExport()}
              disabled={isBusy}
            >
              <Download className="w-4 h-4" />
              {runningTask === "export" ? "書き出し中…" : "データを書き出す"}
            </Button>
            {exportResult && (
              <Button
                variant="outline"
                className="gap-2"
                onClick={() => void handleOpenFolder("exports")}
                disabled={isBusy}
              >
                <FolderOpen className="w-4 h-4" />
                書き出し先フォルダを開く
              </Button>
            )}
          </div>
          <div role="status" aria-live="polite" data-testid="migration-export-status">
            {runningTask === "export" && (
              <p className="text-xs text-muted-foreground">
                書き出しているよ。少し待ってね…
              </p>
            )}
            {exportOutcome && (
              <p
                className={
                  exportOutcome.kind === "error"
                    ? "text-xs text-destructive"
                    : "text-xs text-foreground"
                }
              >
                {exportOutcome.text}
              </p>
            )}
            {exportResult && (
              <dl
                className="mt-1 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs"
                data-testid="migration-export-result"
              >
                <dt className="text-muted-foreground">ファイル名</dt>
                <dd className="break-all">{exportResult.fileName}</dd>
                <dt className="text-muted-foreground">入れたファイル</dt>
                <dd>
                  {exportResult.fileCount}件（ニュース{exportResult.articleCount}件・
                  アーカイブ{exportResult.archiveCount}件・
                  {formatMigrationBytes(exportResult.totalBytes)}）
                </dd>
              </dl>
            )}
          </div>
        </CardContent>
      </Card>

      <Card className="border-0 shadow-sm" data-testid="migration-import-card">
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-base text-[var(--yuuko-green)]">
            <Upload className="w-5 h-5" />
            データの読み込み
          </CardTitle>
        </CardHeader>
        <CardContent className="pt-0 space-y-3">
          <p className="text-xs text-muted-foreground leading-relaxed">
            前のPCで書き出したZIPを、imports フォルダに置いてから選んでね。読み込むと今のデータはすべて置き換わるよ（読み込む前に自動でバックアップを残すよ）。
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              className="gap-2"
              onClick={() => void handleOpenFolder("imports")}
              disabled={isBusy}
            >
              <FolderOpen className="w-4 h-4" />
              imports フォルダを開く
            </Button>
            <Button
              variant="outline"
              className="gap-2"
              onClick={() => void loadCandidates()}
              disabled={isBusy}
            >
              <RefreshCw className="w-4 h-4" />
              一覧を更新
            </Button>
          </div>

          {folderError && (
            <p role="alert" className="text-xs text-destructive" data-testid="migration-folder-error">
              {folderError}
            </p>
          )}

          {runningTask === "list" && (
            <p className="text-xs text-muted-foreground" role="status">
              一覧を読み込んでいるよ…
            </p>
          )}
          {listError && (
            <p role="alert" className="text-xs text-destructive" data-testid="migration-list-error">
              {listError}
            </p>
          )}

          {listLoaded && !listError && candidates.length === 0 && (
            <div
              className="rounded-lg border border-dashed border-border bg-muted/30 p-4 text-center"
              data-testid="migration-import-empty"
            >
              <p className="text-sm text-foreground">
                imports フォルダに移行用ZIPを置いてね
              </p>
              <p className="mt-1 text-xs text-muted-foreground">
                「imports フォルダを開く」でフォルダを開いて、yuuko_transfer_ で始まるZIPを入れたら「一覧を更新」を押してね。
              </p>
            </div>
          )}

          {candidates.length > 0 && (
            <ul className="divide-y divide-border rounded-lg border border-border" data-testid="migration-import-list">
              {candidates.map((candidate) => (
                <li
                  key={candidate.fileName}
                  className="flex items-center justify-between gap-3 px-3 py-2"
                >
                  <div className="min-w-0">
                    <p className="text-sm text-foreground break-all">
                      {candidate.fileName}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      作成日時 {formatMigrationCreatedAt(candidate.createdAt)}・
                      {formatMigrationBytes(candidate.sizeBytes)}
                    </p>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    className="shrink-0"
                    aria-label={`${candidate.fileName} を読み込む`}
                    onClick={() => setConfirmTarget(candidate)}
                    disabled={isBusy}
                  >
                    読み込む
                  </Button>
                </li>
              ))}
            </ul>
          )}

          <div role="status" aria-live="polite" data-testid="migration-import-status">
            {importOutcome && (
              <p
                className={
                  importOutcome.kind === "error"
                    ? "text-xs text-destructive"
                    : "text-xs text-foreground"
                }
              >
                {importOutcome.text}
              </p>
            )}
          </div>
        </CardContent>
      </Card>

      {/* 取り込みの確認（全置き換え・破壊的操作のため必ず挟む）。 */}
      <AlertDialog
        open={confirmTarget !== null}
        onOpenChange={(open) => {
          if (!open) {
            setConfirmTarget(null);
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>データを読み込みますか？</AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="space-y-2 text-sm text-muted-foreground">
                <p className="break-all">
                  「{confirmTarget?.fileName}」の内容で、今のデータ（設定・ニュース・お気に入り・辞書・友情ランク・報酬・ガチャ・アーカイブ）がすべて置き換わります。
                </p>
                <p>今のデータは、読み込みの前に自動でバックアップを1つ残します。</p>
                <p>読み込みが終わったら、アプリを再起動します。</p>
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>キャンセル</AlertDialogCancel>
            <AlertDialogAction onClick={() => void handleConfirmImport()}>
              置き換えて読み込む
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* 取り込み中は閉じられないダイアログで画面全体を覆い、保存などの他の操作を止める。 */}
      <AlertDialog open={isImporting}>
        <AlertDialogContent
          onEscapeKeyDown={(event) => event.preventDefault()}
          data-testid="migration-importing-dialog"
        >
          <AlertDialogHeader>
            <AlertDialogTitle>データを読み込んでいるよ</AlertDialogTitle>
            <AlertDialogDescription>
              終わるまでそのまま待ってね。読み込み中は設定の保存などはできないよ。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="flex justify-center py-2">
            <span className="h-6 w-6 animate-spin rounded-full border-2 border-[var(--yuuko-green)] border-t-transparent" />
          </div>
        </AlertDialogContent>
      </AlertDialog>

      {/* 取り込み完了。表示中の値・常駐処理は取り込み前のデータのままなので、再起動を勧める（§15.7 restartRequired）。 */}
      <AlertDialog
        open={importResult !== null}
        onOpenChange={(open) => {
          if (!open && runningRef.current !== "restart") {
            handleReloadLater();
          }
        }}
      >
        <AlertDialogContent data-testid="migration-import-done-dialog">
          <AlertDialogHeader>
            <AlertDialogTitle>読み込みが終わったよ！</AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="space-y-2 text-sm text-muted-foreground">
                {importResult && (
                  <p>
                    {importResult.fileCount}件のファイル（ニュース{importResult.articleCount}件・アーカイブ{importResult.archiveCount}件）を読み込んだよ。
                  </p>
                )}
                <p>新しいデータで動かすために、アプリを再起動してね。</p>
                {restartError && (
                  <p role="alert" className="text-destructive" data-testid="migration-restart-error">
                    {restartError}
                  </p>
                )}
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <Button
              variant="outline"
              onClick={handleReloadLater}
              disabled={runningTask === "restart"}
            >
              あとで（画面だけ読み込み直す）
            </Button>
            <Button
              className="bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white gap-2"
              onClick={() => void handleRestart()}
              disabled={runningTask === "restart"}
            >
              <RotateCcw className="w-4 h-4" />
              再起動する
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
