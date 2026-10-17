import { useCallback, useEffect, useState } from "react";
import {
  EMPTY_STATUS,
  getStatus,
  type ImportFinished,
  type ImportProgress,
  type LibraryStatus,
  onImportFinished,
  onImportProgress,
  onLibraryChanged,
  onRootNeeded,
} from "@/shared/ipc/library";

/**
 * Library state for the shell: folder and counters, live import progress, the last batch report,
 * and whether the «Папка для музыки» panel must be shown. `version` bumps on every
 * `library-changed`, so lists refetch.
 */
export function useLibrary(initial: LibraryStatus | null) {
  const [status, setStatus] = useState<LibraryStatus>(initial ?? EMPTY_STATUS);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [report, setReport] = useState<ImportFinished | null>(null);
  const [rootNeeded, setRootNeeded] = useState(false);
  const [version, setVersion] = useState(0);

  const refresh = useCallback(() => {
    getStatus().then(setStatus);
    setVersion((v) => v + 1);
  }, []);

  useEffect(() => {
    if (initial) setStatus(initial);
  }, [initial]);

  useEffect(() => {
    const offs: Array<() => void> = [];
    let alive = true;
    const keep = (p: Promise<() => void>) => p.then((off) => (alive ? offs.push(off) : off()));
    keep(onImportProgress(setProgress));
    keep(
      onImportFinished((r) => {
        setProgress(null);
        setReport(r);
        refresh();
      }),
    );
    keep(onLibraryChanged(refresh));
    keep(onRootNeeded(() => setRootNeeded(true)));
    return () => {
      alive = false;
      for (const off of offs) off();
    };
  }, [refresh]);

  return {
    status,
    setStatus,
    progress,
    report,
    clearReport: useCallback(() => setReport(null), []),
    rootNeeded,
    setRootNeeded,
    version,
    refresh,
  };
}
