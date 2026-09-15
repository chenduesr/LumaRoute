import { useCallback, useEffect, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { proxyAction, readSnapshot, type Snapshot } from "../lib/proxy";
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
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      await refresh();
      if (!stopped) timer = setTimeout(poll, 1000);
    };
    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
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
