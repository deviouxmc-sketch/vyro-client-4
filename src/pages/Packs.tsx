import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, errText } from "../api";
import { Page } from "../components/Sidebar";
import { useStore } from "../store";
import type { PackInfo, PackList, SearchHit, Sort } from "../types";

type Tab = "installed" | "browse";
const fmt = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}K` : String(n));
const size = (b: number) => (b <= 0 ? "folder" : b > 1e6 ? `${(b / 1e6).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1e3))} KB`);

function PackIcon({ src, name }: { src: string | null; name: string }) {
  if (!src) return <div className="mod-icon fallback">{name.slice(0, 1).toUpperCase()}</div>;
  return <img className="mod-icon pixel" src={src} alt="" />;
}

export function Packs({ go }: { go: (p: Page) => void }) {
  const s = useStore();
  const p = s.selected;
  const [tab, setTab] = useState<Tab>("installed");
  const [list, setList] = useState<PackList>({ packs: [], builtin: [] });
  const [busy, setBusy] = useState<string | null>(null);
  const [confirmDel, setConfirmDel] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("downloads");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const reqId = useRef(0);

  const load = useCallback(async () => {
    if (!p) return;
    try { setList(await api.packsList(p.id)); } catch (e) { s.toast(errText(e), "error"); }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p?.id]);
  useEffect(() => { void load(); }, [load]);

  const run = async (label: string, f: () => Promise<unknown>, ok?: string) => {
    setBusy(label);
    try { await f(); await load(); if (ok) s.toast(ok); } catch (e) { s.toast(errText(e), "error"); } finally { setBusy(null); }
  };

  const search = useCallback(async (offset: number) => {
    if (!p) return;
    const my = ++reqId.current;
    setLoading(true);
    try {
      const r = await api.packsSearch(p.id, query, sort, offset);
      if (my !== reqId.current) return;
      setHits((old) => (offset === 0 ? r.hits : [...old, ...r.hits]));
      setTotal(r.total_hits);
    } catch (e) {
      if (my === reqId.current) s.toast(errText(e), "error");
    } finally {
      if (my === reqId.current) setLoading(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p?.id, p?.mc_version, query, sort]);

  useEffect(() => {
    if (tab !== "browse") return;
    const t = setTimeout(() => void search(0), 350);
    return () => clearTimeout(t);
  }, [tab, search]);

  if (!p) {
    return <section className="page fade"><div className="panel empty"><p>Create a profile first.</p><button className="btn primary" onClick={() => go("profiles")}>Go to Profiles</button></div></section>;
  }

  const enabled = list.packs.filter((x) => x.enabled).sort((a, b) => (b.position ?? 0) - (a.position ?? 0)); // top = wins
  const available = list.packs.filter((x) => !x.enabled);

  const row = (x: PackInfo, i: number, on: boolean) => (
    <div key={x.file} className={`panel mod-row ${on ? "" : "off"}`}>
      <PackIcon src={x.icon} name={x.file} />
      <div className="grow">
        <div className="strong">{x.file.replace(/\.zip$/i, "")} {x.is_dir && <span className="chip tiny">folder</span>}</div>
        <div className="muted small clamp">{x.description || "No description"}</div>
        <div className="muted small">{x.pack_format !== null ? `pack format ${x.pack_format} · ` : ""}{size(x.size)}</div>
      </div>
      {on && (
        <>
          <button className="btn small ghost" title="Higher priority" disabled={!!busy || i === 0} onClick={() => void run("Moving", () => api.packsMove(p.id, x.file, true))}>↑</button>
          <button className="btn small ghost" title="Lower priority" disabled={!!busy || i === enabled.length - 1} onClick={() => void run("Moving", () => api.packsMove(p.id, x.file, false))}>↓</button>
        </>
      )}
      <button className={on ? "btn small ghost" : "btn small primary"} disabled={!!busy} onClick={() => void run("Saving", () => api.packsSetEnabled(p.id, x.file, !on))}>{on ? "Disable" : "Enable"}</button>
      {confirmDel === x.file ? (
        <button className="btn small danger" disabled={!!busy} onClick={() => { setConfirmDel(null); void run("Deleting", () => api.packsDelete(p.id, x.file), "Pack deleted"); }}>Confirm</button>
      ) : (
        <button className="btn small ghost" disabled={!!busy} onClick={() => setConfirmDel(x.file)}>Delete</button>
      )}
    </div>
  );

  return (
    <section className="page fade">
      <div className="row between wrap gap">
        <div><h1>Resource Packs</h1><p className="muted" style={{ margin: 0 }}>Changes are saved to this profile's settings. Close Minecraft first: it rewrites its settings when it exits.</p></div>
        <div className="row gap-s">
          <select className="select big" value={s.selectedId ?? ""} onChange={(e) => s.setSelectedId(e.target.value)} disabled={!!busy}>
            {s.profiles.map((x) => <option key={x.id} value={x.id}>{x.name} · {x.mc_version}</option>)}
          </select>
          <button className="btn ghost" onClick={() => void api.packsDir(p.id).then((d) => revealItemInDir(d)).catch((e) => s.toast(errText(e), "error"))}>Open folder</button>
        </div>
      </div>

      <div className="tabs">
        <button className={tab === "installed" ? "tab active" : "tab"} onClick={() => setTab("installed")}>Installed ({list.packs.length})</button>
        <button className={tab === "browse" ? "tab active" : "tab"} onClick={() => setTab("browse")}>Browse Modrinth</button>
      </div>
      {busy && <div className="panel busy-bar"><div className="bar"><div className="fill indet" /></div><span className="small muted">{busy}…</span></div>}

      {tab === "installed" && (
        <>
          {list.packs.length === 0 && (
            <div className="panel empty"><p className="muted">No resource packs in this profile yet.</p><button className="btn primary" onClick={() => setTab("browse")}>Browse Modrinth</button>
              <p className="muted small">You can also drop .zip packs into the folder (Open folder).</p></div>
          )}
          {enabled.length > 0 && (
            <>
              <h3>Enabled <span className="muted small" style={{ textTransform: "none", letterSpacing: 0 }}>- packs at the top override the ones below</span></h3>
              <div className="list">{enabled.map((x, i) => row(x, i, true))}</div>
            </>
          )}
          {list.builtin.length > 0 && <p className="muted small">Also active (added by Minecraft or mods, left untouched): {list.builtin.join(", ")}</p>}
          {available.length > 0 && (
            <>
              <h3>Available</h3>
              <div className="list">{available.map((x, i) => row(x, i, false))}</div>
            </>
          )}
        </>
      )}

      {tab === "browse" && (
        <>
          <p className="muted small" style={{ margin: 0 }}>Showing packs for Minecraft {p.mc_version}. "Install" downloads the pack (checked against Modrinth's checksum) and turns it on.</p>
          <div className="row gap-s">
            <input className="input" placeholder="Search resource packs…" value={query} onChange={(e) => setQuery(e.target.value)} />
            <select className="select sort" value={sort} onChange={(e) => setSort(e.target.value as Sort)}>
              <option value="downloads">Most downloaded</option>
              <option value="relevance">Relevance</option>
              <option value="follows">Most followed</option>
              <option value="updated">Recently updated</option>
              <option value="newest">Newest</option>
            </select>
          </div>
          <div className="list">
            {hits.map((h) => (
              <div key={h.project_id} className="panel mod-row">
                <PackIcon src={h.icon_url} name={h.title} />
                <div className="grow">
                  <div className="strong">{h.title} <span className="muted small">by {h.author} · {fmt(h.downloads)} downloads</span></div>
                  <div className="muted small clamp">{h.description}</div>
                </div>
                <button className="btn small primary" disabled={!!busy}
                  onClick={() => void run("Installing", () => api.packsInstall(p.id, h.slug, true), `Installed and enabled ${h.title}`)}>Install</button>
              </div>
            ))}
            {!loading && hits.length === 0 && <div className="panel empty"><p className="muted">No packs found for Minecraft {p.mc_version}.</p></div>}
            {hits.length < total && <button className="btn ghost" disabled={loading} onClick={() => void search(hits.length)}>{loading ? "Loading…" : `Load more (${total - hits.length} left)`}</button>}
            {loading && hits.length === 0 && <p className="muted">Searching…</p>}
          </div>
        </>
      )}
    </section>
  );
}
