"use client";

/**
 * 設定画面「データ管理」タブのアーカイブ管理パネル（判断台帳 D26 / 画面詳細設計書 SCR-003 / データ設計書 §14.8）。
 *
 * - 月別アーカイブを新しい月から一覧し、件数・容量（MB）・削除ボタンを出す（list_archive_months）。
 * - 削除できる時期かどうか（deletable）は Rust が返す値に従い、画面では判定しない。まだの月はボタンを非活性にする。
 * - 削除は、事前確認（get_archive_month_delete_preview）で件数・容量を示し「元に戻せない」確認を経てから実行する。
 *   アーカイブにしか無いお気に入りを含む月などは事前確認がエラーになるため、エラーコードから固定文言を出す。
 * - 削除後は一覧を読み直す。
 *
 * ZIPの特定・検証・削除はすべて Rust 側。ここは年月（YYYY-MM）だけを渡し、パス・Rust のエラー文は表示しない。
 */

import * as React from "react";
import { Archive, RefreshCw, Trash2 } from "lucide-react";
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
  deleteArchiveMonth,
  getArchiveMonthDeletePreview,
  listArchiveMonths,
  type ArchiveMonthDeletePreviewDto,
  type ArchiveMonthDto,
} from "@/lib/tauri/articles";
import {
  archiveManageErrorMessage,
  formatArchiveMonthLabel,
  formatArchiveSizeMb,
} from "@/lib/archive-manage-display.mjs";

/** パネル内で同時に1つだけ走らせる操作。 */
type ArchiveTask = "list" | "preview" | "delete";

type Outcome = { kind: "success" | "error" | "info"; text: string };

const PREVIEW_NOTICE =
  "プレビュー表示では使えないよ。アプリで開いたときに使ってね。";

const consoleErrorKind = (error: unknown) =>
  error instanceof Error ? error.name : typeof error;

const errorCodeOf = (error: unknown) =>
  typeof error === "object" && error !== null
    ? (error as { code?: unknown }).code
    : undefined;

export function ArchiveManagePanel() {
  const [months, setMonths] = React.useState<ArchiveMonthDto[]>([]);
  const [listState, setListState] = React.useState<
    "idle" | "loaded" | "preview" | "error"
  >("idle");
  const [runningTask, setRunningTask] = React.useState<ArchiveTask | null>(
    null
  );
  const [busyMonth, setBusyMonth] = React.useState<string | null>(null);
  const [outcome, setOutcome] = React.useState<Outcome | null>(null);
  const [confirmTarget, setConfirmTarget] =
    React.useState<ArchiveMonthDeletePreviewDto | null>(null);

  // state 更新前の連打でも二重実行しないよう、同期的に参照できる ref でも実行中を保持する。
  const runningRef = React.useRef<ArchiveTask | null>(null);
  const isMountedRef = React.useRef(true);

  const begin = (task: ArchiveTask, month: string | null = null) => {
    if (runningRef.current) {
      return false;
    }
    runningRef.current = task;
    setRunningTask(task);
    setBusyMonth(month);
    return true;
  };

  const finish = () => {
    runningRef.current = null;
    if (isMountedRef.current) {
      setRunningTask(null);
      setBusyMonth(null);
    }
  };

  // 一覧の読み込み本体。実行中の判定は呼び出し側で行う（削除後の読み直しでも使うため）。
  const fetchMonths = React.useCallback(async () => {
    try {
      const list = await listArchiveMonths();
      if (!isMountedRef.current) {
        return;
      }
      if (list === null) {
        setMonths([]);
        setListState("preview");
        return;
      }
      setMonths(list);
      setListState("loaded");
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.error(
        "Failed to list archive months via tauri command:",
        consoleErrorKind(error)
      );
      setListState("error");
    }
  }, []);

  const loadMonths = React.useCallback(async () => {
    if (runningRef.current) {
      return;
    }
    runningRef.current = "list";
    setRunningTask("list");
    try {
      await fetchMonths();
    } finally {
      runningRef.current = null;
      if (isMountedRef.current) {
        setRunningTask(null);
      }
    }
  }, [fetchMonths]);

  React.useEffect(() => {
    isMountedRef.current = true;
    void loadMonths();
    return () => {
      isMountedRef.current = false;
    };
  }, [loadMonths]);

  // 「削除」: まず事前確認で件数・容量と削除可否を確かめ、確認ダイアログを出す（ここでは何も消さない）。
  const handlePreview = async (month: string) => {
    if (!begin("preview", month)) {
      return;
    }
    setOutcome(null);
    try {
      const preview = await getArchiveMonthDeletePreview({ month });
      if (!isMountedRef.current) {
        return;
      }
      if (!preview) {
        setOutcome({ kind: "info", text: PREVIEW_NOTICE });
        return;
      }
      setConfirmTarget(preview);
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.error(
        "Failed to preview archive month delete via tauri command:",
        consoleErrorKind(error)
      );
      setOutcome({
        kind: "error",
        text: archiveManageErrorMessage("preview", error),
      });
      // 一覧が古い（月が既に無い）ときは読み直して表示を合わせる。
      if (errorCodeOf(error) === "NOT_FOUND_ERROR") {
        await fetchMonths();
      }
    } finally {
      finish();
    }
  };

  // 確認ダイアログで「削除する」を押した後だけ呼ばれる。
  const handleConfirmDelete = async () => {
    const target = confirmTarget;
    setConfirmTarget(null);
    if (!target || !begin("delete", target.month)) {
      return;
    }
    setOutcome(null);
    const label = formatArchiveMonthLabel(target.month);
    try {
      const result = await deleteArchiveMonth({ month: target.month });
      if (!isMountedRef.current) {
        return;
      }
      if (!result) {
        setOutcome({ kind: "info", text: PREVIEW_NOTICE });
        return;
      }
      // ZIP を消せなかった（cleanupPending）ときも一覧からは消えており、表示・動作に影響しないため成功として扱う。
      const pendingNote = result.cleanupPending
        ? "ファイルの片付けが一部終わらなかったけど、表示や動作には影響ないよ。"
        : "";
      setOutcome({
        kind: "success",
        text: `${label}のアーカイブ（${result.deletedArticleCount}件）を削除したよ。${pendingNote}`,
      });
      await fetchMonths();
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.error(
        "Failed to delete archive month via tauri command:",
        consoleErrorKind(error)
      );
      setOutcome({
        kind: "error",
        text: archiveManageErrorMessage("delete", error),
      });
      if (errorCodeOf(error) === "NOT_FOUND_ERROR") {
        await fetchMonths();
      }
    } finally {
      finish();
    }
  };

  const isBusy = runningTask !== null;

  return (
    <>
      <Card
        id="archive-manage"
        className="border-0 shadow-sm"
        data-testid="archive-manage-card"
      >
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-base text-[var(--yuuko-green)]">
            <Archive className="w-5 h-5" />
            アーカイブ管理
          </CardTitle>
        </CardHeader>
        <CardContent className="pt-0 space-y-3">
          <p className="text-xs text-muted-foreground leading-relaxed">
            古いニュースは月ごとにまとめて保管しているよ。容量が気になるときは、古い月から削除できるよ。削除したアーカイブは元に戻せないよ（お気に入りや復元したニュースは残るよ）。
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              className="gap-2"
              onClick={() => void loadMonths()}
              disabled={isBusy}
            >
              <RefreshCw className="w-4 h-4" />
              アーカイブを読み直す
            </Button>
          </div>

          {runningTask === "list" && (
            <p className="text-xs text-muted-foreground" role="status">
              一覧を読み込んでいるよ…
            </p>
          )}
          {listState === "error" && (
            <p
              role="alert"
              className="text-xs text-destructive"
              data-testid="archive-manage-list-error"
            >
              {archiveManageErrorMessage("list", null)}
            </p>
          )}
          {listState === "preview" && (
            <p className="text-xs text-muted-foreground">{PREVIEW_NOTICE}</p>
          )}
          {listState === "loaded" && months.length === 0 && (
            <div
              className="rounded-lg border border-dashed border-border bg-muted/30 p-4 text-center"
              data-testid="archive-manage-empty"
            >
              <p className="text-sm text-foreground">
                アーカイブはまだないよ
              </p>
              <p className="mt-1 text-xs text-muted-foreground">
                取得から30日たったニュースが、月ごとにまとめられるよ。
              </p>
            </div>
          )}

          {months.length > 0 && (
            <ul
              className="divide-y divide-border rounded-lg border border-border"
              data-testid="archive-manage-list"
            >
              {months.map((month) => {
                const label = formatArchiveMonthLabel(month.month);
                return (
                  <li
                    key={month.month}
                    className="flex items-center justify-between gap-3 px-3 py-2"
                    data-testid={`archive-month-${month.month}`}
                  >
                    <div className="min-w-0">
                      <p className="text-sm text-foreground">{label}</p>
                      <p className="text-xs text-muted-foreground">
                        {month.articleCount}件・{formatArchiveSizeMb(month.sizeBytes)}
                      </p>
                      {!month.deletable && (
                        <p className="text-[11px] text-muted-foreground">
                          最近の月はまだ削除できないよ
                        </p>
                      )}
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      className="shrink-0 gap-1"
                      aria-label={`${label}のアーカイブを削除`}
                      onClick={() => void handlePreview(month.month)}
                      disabled={isBusy || !month.deletable}
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                      {busyMonth === month.month && runningTask === "delete"
                        ? "削除中…"
                        : "削除"}
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}

          <div
            role="status"
            aria-live="polite"
            data-testid="archive-manage-status"
          >
            {outcome && (
              <p
                className={
                  outcome.kind === "error"
                    ? "text-xs text-destructive"
                    : "text-xs text-foreground"
                }
              >
                {outcome.text}
              </p>
            )}
          </div>
        </CardContent>
      </Card>

      {/* 削除の確認（元に戻せない操作のため必ず挟む）。件数・容量は事前確認の値を出す。 */}
      <AlertDialog
        open={confirmTarget !== null}
        onOpenChange={(open) => {
          if (!open) {
            setConfirmTarget(null);
          }
        }}
      >
        <AlertDialogContent data-testid="archive-delete-confirm-dialog">
          <AlertDialogHeader>
            <AlertDialogTitle>
              {confirmTarget
                ? `${formatArchiveMonthLabel(confirmTarget.month)}のアーカイブを削除しますか？`
                : "アーカイブを削除しますか？"}
            </AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="space-y-2 text-sm text-muted-foreground">
                {confirmTarget && (
                  <p data-testid="archive-delete-confirm-summary">
                    {confirmTarget.articleCount}件 /{" "}
                    {formatArchiveSizeMb(confirmTarget.sizeBytes)}
                    のアーカイブが削除され、元に戻せません。
                  </p>
                )}
                {confirmTarget && confirmTarget.keptArticleCount > 0 && (
                  <p>
                    お気に入りや復元したニュースなど{confirmTarget.keptArticleCount}
                    件は、通常のニュースとして残ります。
                  </p>
                )}
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>キャンセル</AlertDialogCancel>
            <AlertDialogAction onClick={() => void handleConfirmDelete()}>
              削除する
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
