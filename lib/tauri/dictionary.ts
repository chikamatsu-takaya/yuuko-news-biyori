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

export type ExplainSelectedTermParams = {
  articleId: string;
  selectedText: string;
};

export type SaveDictionaryEntryParams = {
  entry: DictionaryEntryDto;
};

export const explainSelectedTerm = async (
  params: ExplainSelectedTermParams
): Promise<DictionaryEntryDto | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<DictionaryEntryDto>("explain_selected_term", { params });
};

export const saveDictionaryEntry = async (
  params: SaveDictionaryEntryParams
): Promise<DictionaryEntryDto> => {
  if (!isTauriRuntime()) {
    return params.entry;
  }

  return invoke<DictionaryEntryDto>("save_dictionary_entry", { params });
};
