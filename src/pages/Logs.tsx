import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, errText } from "../api";
import { Page } from "../components/Sidebar";
import { useStore } from "../store";
import type { ReportDetail, ReportMeta } from "../types";

type Tab = "crash" | "log";
const when = (ms: number) => new Date(ms).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });

function lineClass(l: string): string {
  if (/\b(ERROR|FATAL)\b|Exception|Caused by/.test(l)) return "err";
  if (/\bWARN\b/.test(l)) return "warn";
  return "";
}

export function Logs({ go }: { go: (p: Page) => void }) {
  const s = useStore();
  const p = s.selected;
  const [tab, setTab] = useState<Tab>("crash");
  const [reports, setReports] = useState<ReportMeta[]>([]);
  const [name, setName] = useState<string | null>(null);
  const [detail, setDetail] = useState<ReportDetail | null>(null);
  const [full, setFull] = useState(false);
  const [lines, setLines] = useState(300);
  const [log, setLog] = useState("");
  const logRef = useRef<HTMLPreElement>(null);

  // arriving from the "Minecraft crashed" banner: show that profile and report
  useEffect(() => {
    if (s.crash && s.crash.profileId !== s.selectedId) s.setSelectedId(s.crash.profileId);
    if (s.crash && !s.crash.report) setTab("log");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!p) return;
    api.crashList(p.id).then((r) => {
      setReports(r);
      const wanted = s.crash && s.crash.profileId === p.id ? s.crash.report?.name : null;
      setName(wanted && r.some((x) => x.name === wanted) ? wanted : r[0]?.name ?? null);
    }).catch((e) => s.toast(errText(e), "error"));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p?.id]);

  useEffect(() => {
    setFull(false);
    if (!p || !name) { setDetail(null); return; }
    api.crashRead(p.id, name).then(setDetail).catch((e) => s.toast(errText(e), "error"));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p?.id, name]);

  const loadLog = useCallback(() => {
    if (!p) return;
    api.gameLog(p.id, lines).then((t) => {
      setLog(t);
      setTimeout(() => { if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight; }, 0);
    }).catch((e) => s.toast(errText(e), "error"));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p?.id, lines]);

  useEffect(() => { if (tab === "log") loadLog(); }, [tab, loadLog]);

  const copy = async (text: string) => {
    try { await navigator.clipboard.writeText(text); s.toast("Copied to clipboard"); }
    catch { s.toast("Could not copy. Select the text and copy it by hand.", "error"); }
  };

  if (!p) {
    return <section className="page fade"><div className="panel empty"><p>Create a profile first.</p><button className="btn primary" onClick={() => go("profiles")}>Go to Profiles</button></div></section>;
  }

  return (
    <section className="page fade">
      <div className="row between wrap gap">
        <div><h1>Logs</h1><p className="muted" style={{ margin: 0 }}>Crash reports and the game log for each profile.</p></div>
        <select className="select big" value={s.selectedId ?? ""} onChange={(e) => s.setSelectedId(e.target.value)}>
          {s.profiles.map((x) => <option key={x.id} value={x.id}>{x.name} · {x.mc_version} · {x.loader}</option>)}
        </select>
      </div>

      <div className="tabs">
        <button className={tab === "crash" ? "tab active" : "tab"} onClick={() => setTab("crash")}>Crash reports ({reports.length})</button>
        <button className={tab === "log" ? "tab active" : "tab"} onClick={() => setTab("log")}>Latest log</button>
      </div>

      {tab === "crash" && (
        reports.length === 0 ? (
          <div className="panel empty"><p className="muted">No crash reports for this profile. That's good news.</p></div>
        ) : (
          <div className="logs-grid">
            <div className="list">
              {reports.map((r) => (
                <button key={r.name} className={`panel report-item ${r.name === name ? "sel" : ""}`} onClick={() => setName(r.name)}>
                  <div className="strong small">{when(r.modified_ms)}</div>
                  <div className="muted small clamp">{r.name}</div>
                </button>
              ))}
            </div>
            <div className="list">
              {detail && (
                <>
                  <div className="panel">
                    <div className="muted small">{detail.summary.time || when(detail.modified_ms)}</div>
                    <h3 style={{ textTransform: "none", letterSpacing: 0, color: "var(--text)", fontSize: 16, margin: "4px 0 8px" }}>{detail.summary.description || "Crash report"}</h3>
                    {detail.summary.exception && <pre className="log mono-err">{detail.summary.exception}</pre>}
                    <div className="row gap-s wrap top">
                      <button className="btn small ghost" onClick={() => void copy(detail.text)}>Copy full report</button>
                      <button className="btn small ghost" onClick={() => void revealItemInDir(detail.path).catch((e) => s.toast(errText(e), "error"))}>Show in Finder</button>
                      <button className="btn small ghost" onClick={() => setFull((v) => !v)}>{full ? "Hide full report" : "Show full report"}</button>
                    </div>
                  </div>

                  <div className="panel">
                    <h3>What probably went wrong</h3>
                    {detail.summary.hints.length === 0 ? (
                      <p className="muted">VYRO doesn't recognise this one. The exception above and the stack trace below are the best clues; paste them into a search or ask for help, and note which mods you had installed.</p>
                    ) : detail.summary.hints.map((h) => (
                      <div key={h.title} className="hint-card"><div className="strong">{h.title}</div><div className="muted small">{h.advice}</div></div>
                    ))}
                    {detail.summary.suspected.length > 0 && (
                      <>
                        <h3 style={{ marginTop: 14 }}>Suspected mods</h3>
                        <ul className="plain">{detail.summary.suspected.map((m) => <li key={m}>{m}</li>)}</ul>
                      </>
                    )}
                  </div>

                  <div className="panel">
                    <h3>{full ? "Full report" : "Stack trace (start)"}</h3>
                    <pre className="log tall">{full ? detail.text : detail.summary.head}</pre>
                  </div>
                </>
              )}
            </div>
          </div>
        )
      )}

      {tab === "log" && (
        <div className="panel">
          <div className="row between wrap gap-s">
            <div className="row gap-s">
              <select className="select" style={{ width: 150 }} value={lines} onChange={(e) => setLines(Number(e.target.value))}>
                {[100, 300, 800, 2000].map((n) => <option key={n} value={n}>Last {n} lines</option>)}
              </select>
              <button className="btn small ghost" onClick={loadLog}>Refresh</button>
            </div>
            <div className="row gap-s">
              <button className="btn small ghost" disabled={!log} onClick={() => void copy(log)}>Copy</button>
              <button className="btn small ghost" onClick={() => void api.instanceDir(p.id).then((d) => revealItemInDir(d + "/logs/latest.log")).catch((e) => s.toast(errText(e), "error"))}>Show in Finder</button>
            </div>
          </div>
          <pre className="log tall" ref={logRef}>
            {log ? log.split("\n").map((l, i) => <div key={i} className={lineClass(l)}>{l}</div>) : "No log yet for this profile. Launch the game once and it appears here."}
          </pre>
        </div>
      )}
    </section>
  );
}
