"use client";

/**
 * 定期取得（Rust の news_scheduler）で記事が更新されたとき、ニュース一覧を読み直すための版番号フック。
 *
 * - 一覧画面（MainScreen / NewsListScreen）はマウント時にだけ一覧を読み込むため、Page 側で
 *   この版番号を key に渡して作り直し、読み直しを起こす（画面側のコードは変えない）。
 * - `news-refreshed` を受けても、その場では作り直さない。表示中の一覧が急に読み込み直しになると
 *   操作の邪魔になるため、「次にウィンドウが表示に戻ったとき」に1回だけ反映する。
 *   非表示中に届いた場合も同じで、再表示した時点で最新になる。
 * - 画面を切り替えた場合は一覧が作り直されるため、その時点で最新が読み込まれる。
 * - タイマーやポーリングは持たない（イベント購読と表示切替の検知だけ）。
 */
import { useEffect, useRef, useState } from "react";
import { listenNewsRefreshed } from "@/lib/tauri/news";

export const useNewsListRevision = (isWindowVisible: boolean): number => {
  const [revision, setRevision] = useState(0);
  // 未反映の取得完了があるか。再描画は不要なので ref で持つ。
  const pendingRef = useRef(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    listenNewsRefreshed(() => {
      if (!disposed) {
        pendingRef.current = true;
      }
    })
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch((error) => {
        // 購読できなくても、画面切替やアプリ再起動で一覧は最新になるため警告だけ残す。
        console.warn("Failed to listen news-refreshed for news list:", error);
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 非表示→表示へ戻ったときだけ、未反映の取得完了を一覧へ反映する。
  useEffect(() => {
    if (isWindowVisible && pendingRef.current) {
      pendingRef.current = false;
      setRevision((current) => current + 1);
    }
  }, [isWindowVisible]);

  return revision;
};
