import { useCallback, useEffect, useState } from "react";
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
  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setSnapshot(await readSnapshot());
    } catch (e) {
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
          if (!stopped) setSnapshot(payload);
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
    void refresh();
    void setupEvents().catch(() => {
      // The low-frequency refresh below remains available if event setup fails.
    });
    const fallback = setInterval(() => void refresh(), 30_000);
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
