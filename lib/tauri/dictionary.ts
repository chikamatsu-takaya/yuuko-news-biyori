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
    return params.entry;
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
