import { invoke } from "@tauri-apps/api/core";

import { isTauriRuntime } from "@/lib/tauri/settings";

export type DictionaryEntryType = "term" | "phrase" | "key_point";

export type DictionaryEntryDto = {
  entryId: string;
  keyText: string;
  type: DictionaryEntryType;
  shortExplanation: string;
  detailExplanation: string;
  relatedArticleId?: string;
  relatedArticleTitle?: string;
  isStarred: boolean;
  // 辞書へ保存済みか（保存済み辞書の命中・保存結果なら true、未保存の生成結果なら false）。
  // 「辞書保存済み」表示は ★（isStarred）ではなくこれで判定する（画面詳細設計書 §11.4）。
  // 保存時の入力としては Rust 側で参照しない。
  savedInDictionary: boolean;
};

export type DictionaryEntryListItemDto = {
  entryId: string;
  keyText: string;
  type: DictionaryEntryType;
  shortExplanation: string;
  detailExplanation: string;
  relatedArticleId?: string;
  relatedArticleTitle?: string;
  lastViewedAtText?: string;
  // 作成日時（UNIX秒の文字列）。旧データで無い場合は null / 未定義。
  createdAtText?: string;
  // 参照回数。旧データで欠けている場合は 0 または未定義。
  referenceCount?: number;
  memo?: string;
  isStarred: boolean;
};

export type ExplainSelectedTermParams = {
  articleId: string;
  selectedText: string;
};

export type ListDictionaryEntriesParams = {
  keyword?: string;
  type?: DictionaryEntryType;
  starredOnly?: boolean;
};

export type SaveDictionaryEntryParams = {
  entry: DictionaryEntryDto;
};

export type UpdateDictionaryMemoParams = {
  entryId: string;
  memo: string;
};

export type UpdateDictionaryFavoriteParams = {
  entryId: string;
  isStarred: boolean;
};

export type DeleteDictionaryEntryParams = {
  entryId: string;
};

export const explainSelectedTerm = async (
  params: ExplainSelectedTermParams
): Promise<DictionaryEntryDto | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<DictionaryEntryDto>("explain_selected_term", { params });
};

export const listDictionaryEntries = async (
  params: ListDictionaryEntriesParams = {}
): Promise<DictionaryEntryListItemDto[] | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<DictionaryEntryListItemDto[]>("list_dictionary_entries", {
    params,
  });
};

export const saveDictionaryEntry = async (
  params: SaveDictionaryEntryParams
): Promise<DictionaryEntryDto> => {
  if (!isTauriRuntime()) {
    // ブラウザプレビューでは保存できないが、Rust 側の保存結果と同じく「保存済み」として返す。
    return { ...params.entry, savedInDictionary: true };
  }

  return invoke<DictionaryEntryDto>("save_dictionary_entry", { params });
};

export const updateDictionaryMemo = async (
  params: UpdateDictionaryMemoParams
): Promise<DictionaryEntryListItemDto> => {
  if (!isTauriRuntime()) {
    throw new Error("Tauri runtime not found");
  }

  return invoke<DictionaryEntryListItemDto>("update_dictionary_memo", {
    params,
  });
};

export const updateDictionaryFavorite = async (
  params: UpdateDictionaryFavoriteParams
): Promise<DictionaryEntryListItemDto> => {
  if (!isTauriRuntime()) {
    throw new Error("Tauri runtime not found");
  }

  return invoke<DictionaryEntryListItemDto>("update_dictionary_favorite", {
    params,
  });
};

export const deleteDictionaryEntry = async (
  params: DeleteDictionaryEntryParams
): Promise<string> => {
  if (!isTauriRuntime()) {
    throw new Error("Tauri runtime not found");
  }

  return invoke<string>("delete_dictionary_entry", { params });
};
