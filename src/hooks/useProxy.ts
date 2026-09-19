import { useCallback, useEffect, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  proxyAction,
  readSnapshot,
  type ProxyLog,
  type Snapshot,
} from "../lib/proxy";
export function useProxy() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const lastSnapshotSync = useRef(0);
  const nativeSnapshotRevision = useRef(0);
  const refreshRequestRevision = useRef(0);
  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    const requestRevision = ++refreshRequestRevision.current;
    const eventRevision = nativeSnapshotRevision.current;
    try {
      const next = await readSnapshot();
      // A native event that arrived after this read started is newer than the
      // query result. Never let a late response overwrite that pushed state.
      if (
        requestRevision !== refreshRequestRevision.current ||
        eventRevision !== nativeSnapshotRevision.current
      )
        return;
      lastSnapshotSync.current = Date.now();
      setSnapshot(next);
    } catch (e) {
      if (requestRevision === refreshRequestRevision.current)
        setError(String(e));
    }
  }, []);
  useEffect(() => {
    if (!isTauri()) return;
    let stopped = false;
    const unlisteners: UnlistenFn[] = [];
    const setupEvents = async () => {
      const snapshotUnlisten = await listen<Snapshot>(
        "proxy-snapshot",
        ({ payload }) => {
          if (!stopped) {
            nativeSnapshotRevision.current += 1;
            lastSnapshotSync.current = Date.now();
            setSnapshot(payload);
          }
        },
      );
      if (stopped) snapshotUnlisten();
      else unlisteners.push(snapshotUnlisten);
      const logUnlisten = await listen<ProxyLog>("proxy-log", ({ payload }) => {
        if (stopped) return;
        setSnapshot((current) => {
          if (!current) return current;
          const latest = current.logs[current.logs.length - 1];
          if (
            latest?.timestamp === payload.timestamp &&
            latest.level === payload.level &&
            latest.source === payload.source &&
            latest.message === payload.message
          )
            return current;
          return {
            ...current,
            logs: [...current.logs, payload].slice(-1000),
          };
        });
      });
      if (stopped) logUnlisten();
      else unlisteners.push(logUnlisten);
    };
    const syncWhenVisible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    const bootstrap = async () => {
      try {
        // Subscribe before the initial read so a state transition cannot land
        // in the gap between the first snapshot and listener registration.
        await setupEvents();
      } catch {
        // The low-frequency refresh below remains available if event setup fails.
      } finally {
        if (!stopped) await refresh();
      }
    };
    void bootstrap();
    const fallback = setInterval(() => {
      // Native events are the normal synchronization path. Only query when no
      // snapshot or successful foreground read has arrived for a full interval.
      if (Date.now() - lastSnapshotSync.current >= 30_000) void refresh();
    }, 5_000);
    window.addEventListener("focus", syncWhenVisible);
    document.addEventListener("visibilitychange", syncWhenVisible);
    return () => {
      stopped = true;
      clearInterval(fallback);
      window.removeEventListener("focus", syncWhenVisible);
      document.removeEventListener("visibilitychange", syncWhenVisible);
      for (const unlisten of unlisteners) unlisten();
    };
  }, [refresh]);
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(""), 4500);
    return () => clearTimeout(t);
  }, [notice]);
  const run = useCallback(
    async (
      action: string,
      args: Record<string, unknown> = {},
      message = "",
    ) => {
      setBusy(true);
      setError("");
      try {
        const result = await proxyAction(action, args);
        await refresh();
        if (message) setNotice(message);
        else if (typeof result === "string") setNotice(result);
        return { ok: true, result };
      } catch (e) {
        setError(String(e));
        return { ok: false, result: null };
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );
  return { snapshot, error, setError, notice, setNotice, busy, run, refresh };
}
