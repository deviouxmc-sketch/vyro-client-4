import { createContext, ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api, errText, events } from "./api";
import type { AccountList, CrashNotice, GameExit, LogLine, Profile, Progress, Settings } from "./types";

type Phase = "idle" | "preparing" | "running";
interface Toast { id: number; text: string; kind: "ok" | "error" }

interface Store {
  profiles: Profile[];
  accounts: AccountList;
  settings: Settings;
  selectedId: string | null;
  selected: Profile | null;
  setSelectedId: (id: string) => void;
  reload: () => Promise<void>;
  toast: (text: string, kind?: "ok" | "error") => void;
  toasts: Toast[];
  phase: Phase;
  progress: Progress | null;
  logs: LogLine[];
  lastExit: GameExit | null;
  launch: () => Promise<void>;
  crash: CrashNotice | null;
  clearCrash: () => void;
}

const Ctx = createContext<Store | null>(null);
export const useStore = (): Store => {
  const s = useContext(Ctx);
  if (!s) throw new Error("useStore outside provider");
  return s;
};

const MAX_LOG = 800;

export function StoreProvider({ children }: { children: ReactNode }) {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [accounts, setAccounts] = useState<AccountList>({ accounts: [], active: null });
  const [settings, setSettings] = useState<Settings>({ client_id: "", java_path: null });
  const [selectedId, setSelectedId] = useState<string | null>(localStorage.getItem("vyro.selected"));
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [phase, setPhase] = useState<Phase>("idle");
  const [progress, setProgress] = useState<Progress | null>(null);
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [lastExit, setLastExit] = useState<GameExit | null>(null);
  const nextToast = useRef(1);
  const exitSeen = useRef(false);
  const launchedAt = useRef(0);
  const [crash, setCrash] = useState<CrashNotice | null>(null);

  const toast = useCallback((text: string, kind: "ok" | "error" = "ok") => {
    const id = nextToast.current++;
    setToasts((t) => [...t, { id, text, kind }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), kind === "error" ? 7000 : 3500);
  }, []);

  const reload = useCallback(async () => {
    try {
      const [p, a, s] = await Promise.all([api.profiles(), api.accounts(), api.getSettings()]);
      setProfiles(p);
      setAccounts(a);
      setSettings(s);
    } catch (e) {
      toast(errText(e), "error");
    }
  }, [toast]);

  useEffect(() => { void reload(); }, [reload]);

  // keep a valid selection
  useEffect(() => {
    if (profiles.length === 0) return;
    if (!selectedId || !profiles.some((p) => p.id === selectedId)) setSelectedId(profiles[0].id);
  }, [profiles, selectedId]);
  useEffect(() => { if (selectedId) localStorage.setItem("vyro.selected", selectedId); }, [selectedId]);

  useEffect(() => {
    const subs = [
      events.progress((p) => setProgress(p)),
      events.log((l) => setLogs((old) => (old.length >= MAX_LOG ? [...old.slice(old.length - MAX_LOG + 1), l] : [...old, l]))),
      events.exit((x) => {
        exitSeen.current = true;
        if (x.code !== 0) {
          // the game stopped abnormally: look for a crash report it wrote during this run
          api.crashSince(x.profile_id, launchedAt.current)
            .then((r) => setCrash({ profileId: x.profile_id, report: r, code: x.code }))
            .catch(() => setCrash({ profileId: x.profile_id, report: null, code: x.code }));
        } else {
          setCrash(null);
        }
        setPhase("idle");
        setProgress(null);
        setLastExit(x);
        if (x.code !== 0) toast(`Minecraft exited with code ${x.code ?? "unknown"} – check the log below.`, "error");
      }),
    ];
    return () => { subs.forEach((s) => s.then((un) => un())); };
  }, [toast]);

  const selected = useMemo(() => profiles.find((p) => p.id === selectedId) ?? null, [profiles, selectedId]);

  const launch = useCallback(async () => {
    if (!selected) return;
    setLogs([]);
    setLastExit(null);
    exitSeen.current = false;
    launchedAt.current = Date.now();
    setCrash(null);
    setPhase("preparing");
    setProgress({ stage: "Starting", done: 0, total: 1 });
    try {
      await api.launch(selected.id);
      // the game may already have exited before this call returned
      if (!exitSeen.current) setPhase("running");
      setProgress(null);
    } catch (e) {
      setPhase("idle");
      setProgress(null);
      toast(errText(e), "error");
    }
  }, [selected, toast]);

  const value: Store = {
    profiles, accounts, settings, selectedId, selected, setSelectedId, reload, toast, toasts,
    phase, progress, logs, lastExit, launch, crash, clearCrash: () => setCrash(null),
  };
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
