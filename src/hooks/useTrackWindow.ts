import { useCallback, useMemo, useReducer, useRef, useState } from "react";

import { toUiError, type UiError } from "@/lib/errors";
import { libraryApi } from "@/lib/ipc";
import type { TrackQuery, TrackRow } from "@/types/ipc";

/** Rows requested per round trip. The backend caps a page at 500. */
export const PAGE_SIZE = 500;
/** Pages kept in memory: 12 × 500 = 6 000 rows, ~3 MB at this row shape. */
const MAX_PAGES = 12;

export interface TrackWindow {
  /** Total row count for the query, or 0 until the first page lands. */
  total: number;
  /** Row at an absolute index, or `undefined` while its page is in flight. */
  rowAt: (index: number) => TrackRow | undefined;
  loading: boolean;
  error: UiError | null;
  /** Ask for the pages covering `[first, last]`; safe to call every frame. */
  ensureRange: (first: number, last: number) => void;
  reload: () => void;
}

interface PageCache {
  pages: Map<number, TrackRow[]>;
  inflight: Set<number>;
  center: number;
}

/**
 * Windowed access to a query of any size.
 *
 * A 500 000-track library cannot be held in a JS array, so the list is backed by
 * page requests: the virtualiser asks for the rows it is about to paint and this
 * hook fetches exactly the pages that cover them, keeping a bounded cache so
 * scrolling backwards is instant without letting memory grow with the library.
 *
 * Each query (and each manual reload) owns its cache object, so a response that
 * arrives after the user typed another filter lands in a cache nobody reads
 * instead of overwriting the current results.
 */
export function useTrackWindow(query: TrackQuery): TrackWindow {
  const queryKey = useMemo(() => JSON.stringify(query), [query]);
  const [reloadToken, setReloadToken] = useState(0);
  const key = `${queryKey}#${String(reloadToken)}`;

  // A new key means a new query generation. Swapping the cache object during
  // render is the reset: any response still in flight lands in the abandoned
  // cache instead of writing pages that belong to a different query.
  const cacheRef = useRef<PageCache>({ pages: new Map(), inflight: new Set(), center: 0 });
  const cacheKeyRef = useRef(key);
  if (cacheKeyRef.current !== key) {
    cacheKeyRef.current = key;
    cacheRef.current = { pages: new Map(), inflight: new Set(), center: 0 };
  }
  const cache = cacheRef.current;
  const [, forceRender] = useReducer((value: number) => value + 1, 0);
  const [totals, setTotals] = useState<{ key: string; value: number }>({ key, value: 0 });
  const [failures, setFailures] = useState<{ key: string; error: UiError } | null>(null);
  const [busy, setBusy] = useState<{ key: string; value: boolean }>({ key, value: false });

  // The query object is rebuilt by callers on every render; `queryKey` is its
  // identity, which is what these callbacks actually depend on.
  const queryRef = useRef(query);
  queryRef.current = query;

  const loadPage = useCallback(
    (pageIndex: number) => {
      const target = cache;
      const requestKey = key;
      if (target.inflight.has(pageIndex)) {
        return;
      }
      target.inflight.add(pageIndex);
      setBusy({ key: requestKey, value: true });

      void libraryApi
        .search({
          ...queryRef.current,
          limit: PAGE_SIZE,
          offset: pageIndex * PAGE_SIZE,
        })
        .then((page) => {
          target.pages.set(pageIndex, page.items);
          setTotals({ key: requestKey, value: page.total });
          evict(target, target.center);
          forceRender();
        })
        .catch((cause: unknown) => {
          setFailures({ key: requestKey, error: toUiError(cause) });
        })
        .finally(() => {
          target.inflight.delete(pageIndex);
          setBusy({ key: requestKey, value: target.inflight.size > 0 });
        });
    },
    [cache, key],
  );

  const ensureRange = useCallback(
    (first: number, last: number) => {
      if (last < first || first < 0) {
        return;
      }
      cache.center = Math.floor((first + last) / 2 / PAGE_SIZE);
      const firstPage = Math.floor(first / PAGE_SIZE);
      const lastPage = Math.floor(last / PAGE_SIZE);
      for (let page = firstPage; page <= lastPage; page += 1) {
        if (!cache.pages.has(page)) {
          loadPage(page);
        }
      }
    },
    [cache, loadPage],
  );

  const rowAt = (index: number): TrackRow | undefined => {
    if (index < 0) {
      return undefined;
    }
    return cache.pages.get(Math.floor(index / PAGE_SIZE))?.[index % PAGE_SIZE];
  };

  const reload = useCallback(() => {
    setReloadToken((value) => value + 1);
  }, []);

  return {
    total: totals.key === key ? totals.value : 0,
    rowAt,
    loading: busy.key === key ? busy.value : false,
    error: failures !== null && failures.key === key ? failures.error : null,
    ensureRange,
    reload,
  };
}

/** Drop the pages farthest from the viewport until the cache fits. */
function evict(cache: PageCache, center: number): void {
  if (cache.pages.size <= MAX_PAGES) {
    return;
  }
  const keys = [...cache.pages.keys()].sort(
    (left, right) => Math.abs(right - center) - Math.abs(left - center),
  );
  for (const key of keys.slice(0, cache.pages.size - MAX_PAGES)) {
    cache.pages.delete(key);
  }
}
