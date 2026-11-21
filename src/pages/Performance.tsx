import { useEffect, useState } from "react";
import { api, errText } from "../api";
import { Page } from "../components/Sidebar";
import { useStore } from "../store";
import type { PerfOverview, PerfSession, Preset, Recommendation } from "../types";

const PRESETS: { id: Preset; name: string; blurb: string }[] = [
  { id: "low", name: "Low-end", blurb: "Smallest memory use and the shortest render distance. For older or low-RAM computers." },
  { id: "balanced", name: "Balanced", blurb: "Sensible defaults: good distance and effects without eating all your memory." },
  { id: "high", name: "High-end", blurb: "Longer view distance and more memory for powerful computers." },
];

const fmtTime = (ms: number) => new Date(ms).toLocaleString([], { dateStyle: "short", timeStyle: "short" });

export function Performance({ go }: { go: (p: Page) => void }) {
  const s = useStore();
  const p = s.selected;
  const [ov, setOv] = useState<PerfOverview | null>(null);
  const [recs, setRecs] = useState<Partial<Record<Preset, Recommendation>>>({});
  const [sessions, setSessions] = useState<PerfSession[]>([]);
  const [video, setVideo] = useState(true);
  const [busy, setBusy] = useState(false);

  useEffect(() => { api.perfOverview().then(setOv).catch((e) => s.toast(errText(e), "error")); /* eslint-disable-next-line */ }, []);

  useEffect(() => {
    if (!p) return;
    setRecs({});
    for (const pr of PRESETS) {
      api.perfRecommend(p.id, pr.id).then((r) => setRecs((old) => ({ ...old, [pr.id]: r }))).catch(() => undefined);
    }
    api.perfSessions(p.id).then(setSessions).catch(() => setSessions([]));
  }, [p?.id, p?.ram_mb, p?.loader]); // eslint-disable-line react-hooks/exhaustive-deps

  const apply = async (preset: Preset) => {
    if (!p) return;
    setBusy(true);
    try {
      await api.perfApply(p.id, preset, video);
      await s.reload();
      s.toast(`Applied the ${preset} preset to "${p.name}".`);
    } catch (e) {
      s.toast(errText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const installFps = async () => {
    if (!p) return;
    setBusy(true);
    try {
      const r = await api.modsInstallCollection(p.id, "maximum-fps");
      s.toast(`Installed ${r.installed.length} mod(s). ${r.skipped.length ? `${r.skipped.length} skipped (see the Mods tab for details).` : ""}`);
    } catch (e) {
      s.toast(errText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  if (!p) {
    return <section className="page fade"><div className="panel empty"><p>Create a profile first.</p><button className="btn primary" onClick={() => go("profiles")}>Go to Profiles</button></div></section>;
  }

  const modded = p.loader !== "vanilla";
  const best = sessions.length ? Math.max(...sessions.map((x) => x.avgFps)) : 0;

  return (
    <section className="page fade">
      <div className="row between wrap gap">
        <div><h1>Performance</h1><p className="muted" style={{ margin: 0 }}>Presets are starting points tuned to your hardware. VYRO doesn't promise FPS gains; measure your own below.</p></div>
        <select className="select big" value={s.selectedId ?? ""} onChange={(e) => s.setSelectedId(e.target.value)}>
          {s.profiles.map((x) => <option key={x.id} value={x.id}>{x.name} · {x.mc_version} · {x.loader}</option>)}
        </select>
      </div>

      <div className="panel">
        <h3>Your computer</h3>
        {ov ? (
          <div className="row gap wrap">
            <span className="chip">{(ov.total_mem_mb / 1024).toFixed(1)} GB RAM</span>
            <span className="chip">{ov.cpu_threads} CPU threads</span>
            <span className="chip">{ov.os}</span>
            <span className="chip accent">Suggested preset: {PRESETS.find((x) => x.id === ov.suggested)?.name}</span>
            <span className="muted small">This profile now uses {(p.ram_mb / 1024).toFixed(1)} GB.</span>
          </div>
        ) : <p className="muted">Detecting…</p>}
      </div>

      <label className="check" style={{ color: "var(--text)" }}>
        <input type="checkbox" checked={video} onChange={(e) => setVideo(e.target.checked)} />
        Also set video options (render distance, particles, max FPS…) in this profile's options.txt
      </label>

      <div className="cards">
        {PRESETS.map((pr) => {
          const r = recs[pr.id];
          const rec = ov?.suggested === pr.id;
          return (
            <div key={pr.id} className={`panel card ${rec ? "sel" : ""}`}>
              <div className="row between"><h3>{pr.name}</h3>{rec && <span className="chip accent">suggested</span>}</div>
              <p className="muted small">{pr.blurb}</p>
              {r ? (
                <>
                  <ul className="kv">
                    <li><span>Memory</span><b>{(r.ram_mb / 1024).toFixed(1)} GB</b></li>
                    {r.video.filter(([k]) => ["renderDistance", "simulationDistance", "maxFps"].includes(k)).map(([k, v]) => (
                      <li key={k}><span>{k}</span><b>{v}</b></li>
                    ))}
                  </ul>
                  {r.notes.slice(0, -1).map((n) => <p key={n} className="hint warn small">{n}</p>)}
                </>
              ) : <p className="muted small">…</p>}
              <button className="btn primary small top" disabled={busy || !r} onClick={() => void apply(pr.id)}>Apply to "{p.name}"</button>
            </div>
          );
        })}
      </div>

      <div className="panel vyro-banner">
        <div className="grow">
          <div className="strong">Performance mods</div>
          <div className="muted small">{modded ? "Installs the Maximum FPS bundle (Sodium, Lithium, FerriteCore…) for this profile's version." : "Vanilla profiles don't use mods. Switch the profile to Fabric or Quilt to use performance mods."}</div>
        </div>
        <button className="btn primary small" disabled={busy || !modded} onClick={() => void installFps()}>Install Maximum FPS</button>
      </div>

      <div className="panel">
        <h3>Your sessions (recorded by the VYRO in-game mod)</h3>
        {sessions.length === 0 ? (
          <p className="muted">No sessions yet. Install the VYRO mod on this profile (Mods tab) and play for a few minutes; each session's average FPS shows up here so you can compare presets and mods yourself.</p>
        ) : (
          <table className="table">
            <thead><tr><th>When</th><th>Minecraft</th><th>Length</th><th>Avg FPS</th><th>1% low</th><th>Max memory</th><th>Mods</th></tr></thead>
            <tbody>
              {sessions.slice(0, 12).map((x) => (
                <tr key={x.startedAt}>
                  <td>{fmtTime(x.startedAt)}</td><td>{x.mcVersion || "–"}</td><td>{Math.max(1, Math.round(x.durationSec / 60))} min</td>
                  <td className={x.avgFps === best ? "best" : ""}>{x.avgFps.toFixed(0)}</td><td>{x.low1Fps.toFixed(0)}</td>
                  <td>{(x.maxHeapMb / 1024).toFixed(1)} GB</td><td>{x.mods}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <p className="muted small">Sessions are only comparable when you play similar things (same world, same view distance). Short sessions (under a few minutes) are noisy.</p>
      </div>
    </section>
  );
}
