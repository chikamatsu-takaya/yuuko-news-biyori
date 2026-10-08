import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "@/lib/tauri/settings";

/**
 * データ移行（データ設計書 §15.6 / §15.7）の Tauri command 型付きラッパー。
 * ZIPの作成・検証・自動バックアップ・置き換えはすべて Rust 側が行い、ここは結果を受け取るだけ。
 * 書き出し先（exports/）・取り込み元（imports/）のパスは扱わず、ファイル名だけをやり取りする。
 */

/** export_migration_data の結果。 */
export type MigrationExportResult = {
  /** 書き出したZIPのファイル名だけ（保存先はアプリデータ直下の exports/）。 */
  fileName: string;
  /** manifest.json を除いた格納ファイル数。 */
  fileCount: number;
  articleCount: number;
  archiveCount: number;
  totalBytes: number;
};

/** list_migration_imports の1件（imports/ に置かれた取り込み候補）。 */
export type MigrationImportCandidate = {
  /** importMigrationData にそのまま渡す値。 */
  fileName: string;
  sizeBytes: number;
  /** ZIP内 manifest の作成日時（RFC 3339）。読めなかった場合は null。取り込めるかどうかは取り込み時に検証される。 */
  createdAt: string | null;
};

/** import_migration_data の結果。 */
export type MigrationImportResult = {
  fileName: string;
  fileCount: number;
  articleCount: number;
  archiveCount: number;
  totalBytes: number;
  /** 取り込み後はアプリの再起動を勧める（現状は常に true）。 */
  restartRequired: boolean;
};

export const exportMigrationData =
  async (): Promise<MigrationExportResult | null> => {
    if (!isTauriRuntime()) {
      return null;
    }

    return invoke<MigrationExportResult>("export_migration_data");
  };

export const listMigrationImports = async (): Promise<
  MigrationImportCandidate[]
> => {
  if (!isTauriRuntime()) {
    return [];
  }

  return invoke<MigrationImportCandidate[]>("list_migration_imports");
};

export const importMigrationData = async (
  fileName: string,
): Promise<MigrationImportResult | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<MigrationImportResult>("import_migration_data", { fileName });
};

/** open_migration_folder で開ける固定フォルダ（書き出し先 exports/・取り込み元 imports/）。 */
export type MigrationFolderKind = "exports" | "imports";

/**
 * 書き出し先・取り込み元の固定フォルダをエクスプローラーで開く（Windows のみ）。
 * 渡すのは種類だけで、パスは Rust 側が固定名から決める（任意のパスは開けない）。
 * 非Tauri（プレビュー）では何もせず false を返す。
 */
export const openMigrationFolder = async (
  kind: MigrationFolderKind,
): Promise<boolean> => {
  if (!isTauriRuntime()) {
    return false;
  }

  await invoke<void>("open_migration_folder", { kind });
  return true;
};

/**
 * アプリを再起動する（取り込み後に、画面と常駐処理を新しいデータで始め直すため。データ設計書 §15.7）。
 * 書き出し・取り込みの実行中は Rust 側が MIGRATION_BUSY で断る。非Tauri（プレビュー）では false を返す。
 */
export const restartApp = async (): Promise<boolean> => {
  if (!isTauriRuntime()) {
    return false;
  }

  await invoke<void>("restart_app");
  return true;
};
