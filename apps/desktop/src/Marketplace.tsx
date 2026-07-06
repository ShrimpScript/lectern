/* Lectern Hub — skills (mine + the community hub). Extracted from App.tsx and
   lazy-loaded: it's never on the startup path, so its code (cards, forms, review
   modal, registry calls) stays out of the boot chunk. */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ctrl, DANGER, Icon, Scroll, WARN, type RegistryEntry, type SkillBundle, type SkillInfo } from "./App";

function SkillCard({ s, onDelete, onEdit, onExport, onPublish, onReenable, onRefine }: { s: SkillInfo; onDelete: (name: string) => void; onEdit: (s: SkillInfo) => void; onExport: (name: string) => void; onPublish?: (name: string) => void; onReenable: (name: string) => void; onRefine: (s: SkillInfo) => void }) {
  const [open, setOpen] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const link = (label: string, onClick: () => void, danger?: boolean): React.ReactNode => (
    <button onClick={onClick} style={{ border: "none", background: "transparent", color: danger ? DANGER : "var(--fg3)", fontSize: 12, cursor: "pointer", padding: 0, fontFamily: "inherit", fontWeight: 600 }}>{label}</button>
  );
  return (
    <div style={{ border: "1px solid var(--bd)", borderRadius: 13, padding: 18, background: "var(--panel)", display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
        <div style={{ width: 36, height: 36, border: "1px solid var(--bd)", borderRadius: 9, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--fg2)" }}><Icon name="agent" size={17} /></div>
        <span style={{ display: "inline-flex", gap: 6 }}>
          {s.paused && <span className="mono" title="This skill failed in most recent runs, so it stopped auto-applying. Re-enable gives it a fresh start." style={{ fontSize: 10, color: WARN, border: `1px solid ${WARN}`, borderRadius: 5, padding: "3px 7px" }}>paused — failing</span>}
          <span className="mono" style={{ fontSize: 10, color: "var(--fg3)", border: "1px solid var(--bd)", borderRadius: 5, padding: "3px 7px" }}>{s.gui ? "replays" : "skill"}</span>
        </span>
      </div>
      <div>
        <div style={{ fontWeight: 700, fontSize: 15 }}>{s.name}</div>
        <div className="mono" style={{ fontSize: 11, color: "var(--fg3)", marginTop: 2 }}>used {s.uses}×{s.ok + s.err > 0 ? ` · ${s.ok} clean / ${s.err} failed` : ""}{s.triggers.length ? ` · ${s.triggers.length} trigger${s.triggers.length === 1 ? "" : "s"}` : ""}</div>
      </div>
      <div style={{ fontSize: 13, lineHeight: 1.5, color: "var(--fg2)" }}>{s.description || "A reusable workflow Lectern applies on matching tasks."}</div>
      {(s.steps.length > 0 || s.rules.length > 0) && (
        <>
          <button onClick={() => setOpen((o) => !o)} style={{ alignSelf: "flex-start", border: "none", background: "transparent", color: "var(--fg3)", fontSize: 11.5, cursor: "pointer", padding: 0, fontFamily: "inherit" }}>{open ? "▾" : "▸"} {[s.rules.length ? `${s.rules.length} rule${s.rules.length === 1 ? "" : "s"}` : "", s.steps.length ? `${s.steps.length} step${s.steps.length === 1 ? "" : "s"}` : ""].filter(Boolean).join(" · ")}</button>
          {open && (
            <ol className="mono" style={{ margin: 0, paddingLeft: 18, fontSize: 11.5, color: "var(--fg2)", lineHeight: 1.6, display: "flex", flexDirection: "column", gap: 2 }}>
              {[...s.rules, ...s.steps].map((st, j) => <li key={j} style={{ whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }} title={st}>{st}</li>)}
            </ol>
          )}
        </>
      )}
      <div style={{ display: "flex", alignItems: "center", gap: 14, marginTop: 2, paddingTop: 10, borderTop: "1px solid var(--bd2)" }}>
        {link("Edit", () => onEdit(s))}
        {s.paused && link("Re-enable", () => onReenable(s.name))}
        {s.err > 0 && link("Refine with AI", () => onRefine(s))}
        {link("Export", () => onExport(s.name))}
        {onPublish && link("Publish", () => onPublish(s.name))}
        <span style={{ marginLeft: "auto" }}>
          {confirm ? (
            <span style={{ display: "inline-flex", gap: 10 }}>{link("Delete", () => { onDelete(s.name); setConfirm(false); }, true)}{link("Cancel", () => setConfirm(false))}</span>
          ) : link("Delete", () => setConfirm(true), true)}
        </span>
      </div>
    </div>
  );
}

// Create / edit a skill by hand — the portable, shareable unit. Rules are conventions the
// agent should follow; steps are an ordered procedure (or the actions of a recorded macro).
function SkillForm({ initial, onSave, onClose }: { initial?: SkillInfo; onSave: (s: { name: string; description: string; triggers: string[]; rules: string[]; steps: string[] }) => void; onClose: () => void }) {
  const [name, setName] = useState(initial?.name ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const [triggers, setTriggers] = useState((initial?.triggers ?? []).join(", "));
  const [rules, setRules] = useState((initial?.rules ?? []).join("\n"));
  const [steps, setSteps] = useState((initial?.steps ?? []).join("\n"));
  const lines = (t: string) => t.split("\n").map((x) => x.trim()).filter(Boolean);
  const field: React.CSSProperties = { width: "100%", boxSizing: "border-box", borderRadius: 8, border: "1px solid var(--bd)", background: "var(--bg)", color: "var(--fg)", fontSize: 13.5, padding: "9px 11px", outline: "none", fontFamily: "inherit" };
  const lbl: React.CSSProperties = { fontSize: 12, fontWeight: 600, color: "var(--fg2)", marginBottom: 5, display: "block" };
  const save = () => { if (name.trim()) onSave({ name: name.trim(), description: description.trim(), triggers: triggers.split(",").map((t) => t.trim()).filter(Boolean), rules: lines(rules), steps: lines(steps) }); };
  return (
    <div style={{ position: "fixed", inset: 0, zIndex: 90, background: "var(--backdrop)", display: "flex", alignItems: "center", justifyContent: "center", padding: 24 }}>
      <div className="lectern-msg" style={{ width: "100%", maxWidth: 540, maxHeight: "88vh", overflow: "auto", background: "var(--bg)", border: "1px solid var(--bd)", borderRadius: 16, padding: "24px 26px", boxShadow: "0 30px 80px -20px rgba(0,0,0,.4)", display: "flex", flexDirection: "column", gap: 14 }}>
        <div style={{ fontSize: 18, fontWeight: 700 }}>{initial ? "Edit skill" : "New skill"}</div>
        <div><label style={lbl}>Name</label><input value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Add a settings page" style={field} /></div>
        <div><label style={lbl}>Description</label><input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="One line: what this skill does" style={field} /></div>
        <div><label style={lbl}>Triggers <span style={{ color: "var(--fg3)", fontWeight: 400 }}>(comma-separated; blank = derived from the name)</span></label><input value={triggers} onChange={(e) => setTriggers(e.target.value)} placeholder="settings, preferences, config" style={field} /></div>
        <div><label style={lbl}>Rules <span style={{ color: "var(--fg3)", fontWeight: 400 }}>(conventions to follow, one per line)</span></label><textarea value={rules} onChange={(e) => setRules(e.target.value)} rows={4} placeholder={"Use the existing design tokens\nAdd a test for each change"} style={{ ...field, resize: "vertical", lineHeight: 1.5 }} /></div>
        <div><label style={lbl}>Steps <span style={{ color: "var(--fg3)", fontWeight: 400 }}>(ordered procedure, one per line)</span></label><textarea value={steps} onChange={(e) => setSteps(e.target.value)} rows={4} placeholder={"Create the page component\nWire it into the router"} style={{ ...field, resize: "vertical", lineHeight: 1.5 }} /></div>
        <div style={{ display: "flex", gap: 10, justifyContent: "flex-end", marginTop: 4 }}>
          <button onClick={onClose} style={{ height: 36, padding: "0 16px", borderRadius: 9, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg)", fontSize: 13.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>Cancel</button>
          <button onClick={save} disabled={!name.trim()} style={{ height: 36, padding: "0 20px", borderRadius: 9, border: "none", background: name.trim() ? "var(--btn)" : "var(--bd)", color: name.trim() ? "var(--btnfg)" : "var(--fg3)", fontSize: 13.5, fontWeight: 700, cursor: name.trim() ? "pointer" : "default", fontFamily: "inherit" }}>{initial ? "Save" : "Create skill"}</button>
        </div>
      </div>
    </div>
  );
}

// A browse card for a skill on the community hub. Install opens a review modal
// first — we never import (or run) anything without showing the user its steps.
function CommunityCard({ e, installed, installedVersion, onInstall }: { e: RegistryEntry; installed: boolean; installedVersion?: number; onInstall: (e: RegistryEntry) => void }) {
  const update = installed && installedVersion !== undefined && e.version > installedVersion;
  const label = update ? "Update" : installed ? "Reinstall" : "Review & install";
  if (e.external) {
    // Ecosystem tier: official collections elsewhere — link out, never fake-install.
    return (
      <div style={{ border: "1px solid var(--bd)", borderRadius: 13, padding: 18, background: "var(--panel)", display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
          <div style={{ width: 36, height: 36, border: "1px solid var(--bd)", borderRadius: 9, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--fg2)" }}><Icon name="agent" size={17} /></div>
          <span style={{ display: "inline-flex", gap: 6 }}>
            {e.official && <span className="mono" title={`Official collection from ${e.publisher ?? "its publisher"}.`} style={{ fontSize: 10, color: "var(--fg)", border: "1px solid var(--fg)", borderRadius: 5, padding: "3px 7px", fontWeight: 600 }}>{(e.publisher ?? "official").toLowerCase()}</span>}
            <span className="mono" style={{ fontSize: 10, color: "var(--fg3)", border: "1px solid var(--bd)", borderRadius: 5, padding: "3px 7px" }}>external</span>
          </span>
        </div>
        <div>
          <div style={{ fontWeight: 700, fontSize: 15 }}>{e.name}</div>
          <div style={{ fontSize: 12.5, color: "var(--fg2)", lineHeight: 1.55, marginTop: 5 }}>{e.description}</div>
        </div>
        <button onClick={() => e.source_url && invoke("open_url", { url: e.source_url }).catch(() => {})}
          style={{ marginTop: "auto", height: 32, borderRadius: 8, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg)", fontSize: 12.5, fontWeight: 650, cursor: "pointer", fontFamily: "inherit" }}>
          View source ↗
        </button>
      </div>
    );
  }
  return (
    <div style={{ border: "1px solid " + (update ? "var(--fg)" : "var(--bd)"), borderRadius: 13, padding: 18, background: "var(--panel)", display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
        <div style={{ width: 36, height: 36, border: "1px solid var(--bd)", borderRadius: 9, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--fg2)" }}><Icon name="agent" size={17} /></div>
        <span style={{ display: "inline-flex", gap: 6 }}>
          {e.official && <span className="mono" title="Curated and maintained by the Lectern team." style={{ fontSize: 10, color: "var(--fg)", border: "1px solid var(--fg)", borderRadius: 5, padding: "3px 7px", fontWeight: 600 }}>official</span>}
          <span className="mono" style={{ fontSize: 10, color: "var(--fg3)", border: "1px solid var(--bd)", borderRadius: 5, padding: "3px 7px" }}>{e.kind === "gui" ? "replays" : "skill"}{e.version > 1 ? ` · v${e.version}` : ""}</span>
        </span>
      </div>
      <div>
        <div style={{ fontWeight: 700, fontSize: 15 }}>{e.name}</div>
        <div className="mono" style={{ fontSize: 11, color: "var(--fg3)", marginTop: 2 }}>{e.author ? `by ${e.author}` : "community"}{e.triggers.length ? ` · ${e.triggers.length} trigger${e.triggers.length === 1 ? "" : "s"}` : ""}</div>
      </div>
      <div style={{ fontSize: 13, lineHeight: 1.5, color: "var(--fg2)" }}>{e.description || "A reusable skill from the community hub."}</div>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginTop: 2, paddingTop: 10, borderTop: "1px solid var(--bd2)" }}>
        <button onClick={() => onInstall(e)} style={{ height: 30, padding: "0 13px", borderRadius: 7, border: update ? "none" : "1px solid var(--bd)", background: update ? "var(--btn)" : "transparent", color: update ? "var(--btnfg)" : "var(--fg)", fontSize: 12.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>{label}</button>
        {update ? <span className="mono" style={{ fontSize: 11, color: "var(--fg)" }}>v{e.version} available</span> : installed && <span className="mono" style={{ fontSize: 11, color: "var(--fg3)" }}>installed</span>}
      </div>
    </div>
  );
}

// Review-before-install: fetch the full bundle and SHOW its exact rules + steps
// (which, for replay skills, are literal commands/keystrokes) before importing.
function ReviewModal({ entry, onClose, onInstalled }: { entry: RegistryEntry; onClose: () => void; onInstalled: (name: string) => void }) {
  const [bundle, setBundle] = useState<SkillBundle | null>(null);
  const [verified, setVerified] = useState<boolean | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    invoke<{ bundle: SkillBundle; verified: boolean }>("fetch_registry_skill", { id: entry.id, sha256: entry.sha256 ?? null })
      .then((r) => { setBundle(r.bundle); setVerified(r.verified); })
      .catch((e) => setErr(String(e)));
  }, [entry.id, entry.sha256]);
  const gui = entry.kind === "gui";
  const install = async () => {
    setBusy(true); setErr(null);
    try { const name = await invoke<string>("install_registry_skill", { id: entry.id, sha256: entry.sha256 ?? null }); onInstalled(name); }
    catch (e) { setErr(String(e)); setBusy(false); }
  };
  const section = (label: string, items: string[]) => items.length > 0 && (
    <div>
      <div style={{ fontSize: 12, fontWeight: 600, color: "var(--fg2)", marginBottom: 6 }}>{label}</div>
      <ol className="mono" style={{ margin: 0, paddingLeft: 18, fontSize: 12, color: "var(--fg)", lineHeight: 1.65, display: "flex", flexDirection: "column", gap: 3 }}>
        {items.map((it, i) => <li key={i} style={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{it}</li>)}
      </ol>
    </div>
  );
  return (
    <div style={{ position: "fixed", inset: 0, zIndex: 90, background: "var(--backdrop)", display: "flex", alignItems: "center", justifyContent: "center", padding: 24 }}>
      <div className="lectern-msg" style={{ width: "100%", maxWidth: 580, maxHeight: "88vh", overflow: "auto", background: "var(--bg)", border: "1px solid var(--bd)", borderRadius: 16, padding: "24px 26px", boxShadow: "0 30px 80px -20px rgba(0,0,0,.4)", display: "flex", flexDirection: "column", gap: 14 }}>
        <div>
          <div style={{ fontSize: 18, fontWeight: 700 }}>{entry.name}</div>
          <div className="mono" style={{ fontSize: 11.5, color: "var(--fg3)", marginTop: 3, display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            <span>{entry.author ? `by ${entry.author} · ` : ""}from the community hub</span>
            {entry.official && <span title="Curated and maintained by the Lectern team." style={{ color: "var(--fg)", border: "1px solid var(--fg)", borderRadius: 5, padding: "1px 7px", fontSize: 10.5, fontWeight: 600 }}>official</span>}
            {verified === true && <span title="The downloaded file matches the hub index's sha256." style={{ color: "var(--fg2)", border: "1px solid var(--bd)", borderRadius: 5, padding: "1px 7px", fontSize: 10.5 }}>integrity verified ✓</span>}
            {verified === false && <span title="This index entry predates integrity hashes — the content shown below is exactly what would install; review it carefully." style={{ color: "var(--fg3)", border: "1px dashed var(--bd)", borderRadius: 5, padding: "1px 7px", fontSize: 10.5 }}>unsigned entry</span>}
          </div>
        </div>
        {entry.description && <div style={{ fontSize: 13.5, color: "var(--fg2)", lineHeight: 1.55 }}>{entry.description}</div>}
        <div style={{ fontSize: 12.5, color: "var(--fg2)", lineHeight: 1.6, border: "1px solid var(--bd)", borderRadius: 10, padding: "11px 13px", background: "var(--panel)" }}>
          {gui
            ? <>This is a <b>replay skill</b>: installing lets Lectern perform the literal clicks/keystrokes below on your machine. Review every step before installing.</>
            : <>Review the rules and steps below — this is exactly what Lectern will apply when the skill matches a task. Nothing runs until then.</>}
        </div>
        {err && <div className="mono" style={{ fontSize: 12, color: DANGER, lineHeight: 1.5 }}>{err}</div>}
        {!bundle && !err ? (
          <div className="mono" style={{ fontSize: 12, color: "var(--fg3)" }}>Loading skill…</div>
        ) : bundle && (
          <>
            {bundle.triggers.length > 0 && <div className="mono" style={{ fontSize: 11.5, color: "var(--fg3)" }}>triggers: {bundle.triggers.join(", ")}</div>}
            {bundle.docs && (
              <div>
                <div style={{ fontSize: 12, fontWeight: 600, color: "var(--fg2)", marginBottom: 6 }}>Documentation</div>
                <div style={{ border: "1px solid var(--bd2)", borderRadius: 9, padding: "11px 13px", background: "var(--panel)" }}><MdView text={bundle.docs} /></div>
              </div>
            )}
            {section("Rules", bundle.rules)}
            {section(gui ? "Actions (run on install/use)" : "Steps", bundle.steps)}
          </>
        )}
        <div style={{ display: "flex", gap: 10, justifyContent: "flex-end", marginTop: 4 }}>
          <button onClick={onClose} style={{ height: 36, padding: "0 16px", borderRadius: 9, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg)", fontSize: 13.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>Cancel</button>
          <button onClick={install} disabled={!bundle || busy} style={{ height: 36, padding: "0 20px", borderRadius: 9, border: "none", background: bundle && !busy ? "var(--btn)" : "var(--bd)", color: bundle && !busy ? "var(--btnfg)" : "var(--fg3)", fontSize: 13.5, fontWeight: 700, cursor: bundle && !busy ? "pointer" : "default", fontFamily: "inherit" }}>{busy ? "Installing…" : "Install skill"}</button>
        </div>
      </div>
    </div>
  );
}

// ── Marketplace ──────────────────────────────────────────────────────────────
export function Marketplace({ path, onRefine }: { path: string; onRefine?: (draft: string) => void }) {
  const [tab, setTab] = useState<"mine" | "community">("mine");
  const [items, setItems] = useState<SkillInfo[] | null>(null);
  const [community, setCommunity] = useState<RegistryEntry[] | null>(null);
  const [q, setQ] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [form, setForm] = useState<{ initial?: SkillInfo } | null>(null);
  const [review, setReview] = useState<RegistryEntry | null>(null);
  const [installedVer, setInstalledVer] = useState<Record<string, number>>({});
  const wsPath = path.trim() || "/"; // skills are global; any valid dir opens the store
  const load = () => { invoke<SkillInfo[]>("skills", { path: wsPath }).then(setItems).catch(() => setItems([])); };
  useEffect(load, [path]); // eslint-disable-line react-hooks/exhaustive-deps
  const loadCommunity = () => { setCommunity(null); setMsg(null); invoke<Record<string, number>>("registry_installed").then(setInstalledVer).catch(() => {}); invoke<RegistryEntry[]>("browse_registry").then(setCommunity).catch((e) => { setCommunity([]); setMsg(`Couldn't reach the hub: ${String(e)}`); }); };
  useEffect(() => { if (tab === "community" && community === null) loadCommunity(); }, [tab]); // eslint-disable-line react-hooks/exhaustive-deps
  const saveSkill = async (sk: { name: string; description: string; triggers: string[]; rules: string[]; steps: string[] }) => {
    setMsg(null);
    try { await invoke("create_skill", sk); setForm(null); setMsg(`Saved “${sk.name}”.`); load(); }
    catch (e) { setMsg(`Couldn't save: ${String(e)}`); }
  };
  const reenable = (name: string) => { invoke("reset_skill_stats", { name }).then(load).catch(() => {}); };
  const refine = (sk: SkillInfo) => {
    // Draft (never send) an improvement session — spend stays explicit.
    onRefine?.(
      `Improve my learned skill "${sk.name}" — it failed in ${sk.err} of its last ${sk.ok + sk.err} uses. ` +
      `Current rules: ${sk.rules.join(" | ") || "(none)"}. Current steps: ${sk.steps.join(" | ") || "(none)"}. ` +
      `Diagnose what's likely going wrong, then propose a tightened replacement (same intent, clearer rules/steps) and update the skill.`,
    );
  };
  const exportSkill = async (name: string) => {
    try {
      const json = await invoke<string>("export_skill", { path: wsPath, name });
      const saved = await invoke<string | null>("save_skill_file", { name, content: json });
      if (saved) setMsg(`Exported “${name}” to ${saved}.`);
    } catch (e) { setMsg(`Couldn't export: ${String(e)}`); }
  };
  const publishSkill = async (name: string) => {
    setMsg(`Auditing “${name}” before publish (static rules + a free-model check)…`);
    try {
      const rep = await invoke<{ verdict: "pass" | "warn" | "block"; findings: string[]; model_note: string }>("audit_skill", { path: wsPath, name });
      if (rep.verdict === "block") {
        setMsg(`Publish blocked by the security audit: ${rep.findings.join("; ") || rep.model_note}. Fix the skill and try again (local use is unaffected).`);
        return;
      }
      const note = rep.verdict === "warn" ? ` Audit warnings: ${rep.findings.join("; ")}.` : ` Audit passed (${rep.model_note}).`;
      await invoke<string>("publish_skill", { path: wsPath, name });
      setMsg(`Opened a GitHub pull request for “${name}” in your browser — review and click “Propose new file” to submit it.${note}`);
    } catch (e) { setMsg(`Couldn't publish: ${String(e)}`); }
  };
  const importSkill = async () => {
    setMsg(null);
    try { const name = await invoke<string | null>("import_skill_file"); if (name) { setMsg(`Imported “${name}”.`); load(); } }
    catch (e) { setMsg(`Couldn't import: ${String(e)}`); }
  };
  const sync = async () => {
    if (!path.trim() || busy) return;
    setBusy(true); setMsg(null);
    try { const n = await invoke<number>("sync_skills", { path }); setMsg(`Synced ${n} skill${n === 1 ? "" : "s"} to .claude/skills/ — Claude Code will use them natively.`); }
    catch (e) { setMsg(`Couldn't sync: ${String(e)}`); }
    setBusy(false);
  };
  const del = (name: string) => { invoke("delete_skill", { path: wsPath, name }).then(() => { setMsg(`Deleted “${name}”.`); load(); }).catch((e) => setMsg(`Couldn't delete: ${String(e)}`)); };
  const installed = new Set((items ?? []).map((s) => s.name.toLowerCase()));
  const matches = (name: string, desc: string) => !q.trim() || name.toLowerCase().includes(q.toLowerCase()) || desc.toLowerCase().includes(q.toLowerCase());
  const shown = (items ?? []).filter((s) => matches(s.name, s.description));
  const shownCommunity = (community ?? []).filter((e) => matches(e.name, e.description)).sort((a, b) => Number(b.official ?? false) - Number(a.official ?? false));
  const aBtn = (primary: boolean, disabled = false): React.CSSProperties => ({ height: 34, padding: "0 14px", borderRadius: 8, fontSize: 12.5, fontWeight: 600, fontFamily: "inherit", cursor: disabled ? "default" : "pointer", border: primary ? "none" : "1px solid var(--bd)", background: primary ? "var(--btn)" : "transparent", color: primary ? "var(--btnfg)" : "var(--fg)", opacity: disabled ? 0.5 : 1, whiteSpace: "nowrap" });
  const tabBtn = (key: "mine" | "community", label: string): React.CSSProperties => ({ height: 32, padding: "0 14px", borderRadius: 8, fontSize: 13, fontWeight: 600, fontFamily: "inherit", cursor: "pointer", border: "1px solid " + (tab === key ? "var(--fg)" : "var(--bd)"), background: tab === key ? "var(--fg)" : "transparent", color: tab === key ? "var(--bg)" : "var(--fg2)" });
  return (
    <Scroll>
      <div style={{ maxWidth: 860, margin: "0 auto", padding: "44px 40px", display: "flex", flexDirection: "column", gap: 18 }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-end", gap: 16, flexWrap: "wrap" }}>
          <div>
            <div style={{ fontSize: 26, fontWeight: 800, letterSpacing: "-0.02em" }}>Hub</div>
            <div style={{ fontSize: 14, color: "var(--fg2)", marginTop: 4 }}>{tab === "mine" ? "Your reusable skills — written by hand, recorded, or imported. Lectern applies them on matching tasks and syncs them to Claude Code." : "Skills shared by the community. Browse, review exactly what each does, then install into your brain."}</div>
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            {tab === "mine" ? <>
              <button onClick={importSkill} style={aBtn(false)}>Import</button>
              <button onClick={() => setForm({})} style={aBtn(true)}>New skill</button>
            </> : (
              <button onClick={loadCommunity} disabled={community === null} style={aBtn(false, community === null)}>Refresh</button>
            )}
          </div>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <button onClick={() => setTab("mine")} style={tabBtn("mine", "My skills")}>My skills{items ? ` · ${items.length}` : ""}</button>
          <button onClick={() => setTab("community")} style={tabBtn("community", "Community")}>Community{community ? ` · ${community.length}` : ""}</button>
        </div>
        {tab === "mine"
          ? <div style={{ fontSize: 12.5, color: "var(--fg3)", lineHeight: 1.6 }}>Skills are portable — <b>Export</b> as JSON, or <b>Publish</b> to share on the community hub. <button onClick={sync} disabled={busy || !path.trim()} style={{ border: "none", background: "transparent", color: path.trim() ? "var(--fg)" : "var(--fg3)", textDecoration: "underline", textUnderlineOffset: 2, cursor: path.trim() ? "pointer" : "default", fontSize: 12.5, fontFamily: "inherit", padding: 0 }}>Sync all to Claude Code →</button></div>
          : <div style={{ fontSize: 12.5, color: "var(--fg3)", lineHeight: 1.6 }}>Installing always shows you the skill’s exact steps first — nothing is added or run without your review.</div>}
        {msg && <div className="mono" style={{ fontSize: 12, color: msg.startsWith("Couldn't") ? DANGER : "var(--fg)", lineHeight: 1.5 }}>{msg}</div>}
        <input value={q} onChange={(e) => setQ(e.target.value)} placeholder={tab === "mine" ? "Search your skills…" : "Search the community hub…"} style={{ ...ctrl, height: 38, fontSize: 13 }} />
        {tab === "mine" ? (
          items === null ? (
            <div className="mono" style={{ fontSize: 12, color: "var(--fg3)" }}>Loading…</div>
          ) : shown.length === 0 ? (
            <div style={{ border: "1px solid var(--bd)", borderRadius: 13, background: "var(--panel)", padding: "32px 22px", textAlign: "center" }}>
              <div style={{ fontSize: 15, fontWeight: 700 }}>No skills yet</div>
              <div style={{ fontSize: 13.5, color: "var(--fg2)", marginTop: 8, lineHeight: 1.6, maxWidth: 460, margin: "8px auto 0" }}>Click <span style={{ color: "var(--fg)", fontWeight: 600 }}>New skill</span> to write one, browse the <button onClick={() => setTab("community")} style={{ border: "none", background: "transparent", color: "var(--fg)", fontWeight: 600, cursor: "pointer", fontFamily: "inherit", fontSize: 13.5, padding: 0, textDecoration: "underline", textUnderlineOffset: 2 }}>Community</button> hub, or record a workflow with <span className="mono" style={{ color: "var(--fg)" }}>/record</span> in a chat.</div>
            </div>
          ) : (
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14 }}>
              {shown.map((s, i) => <SkillCard key={i} s={s} onDelete={del} onEdit={(sk) => setForm({ initial: sk })} onExport={exportSkill} onPublish={publishSkill} onReenable={reenable} onRefine={refine} />)}
            </div>
          )
        ) : (
          community === null ? (
            <div className="mono" style={{ fontSize: 12, color: "var(--fg3)" }}>Reaching the hub…</div>
          ) : shownCommunity.length === 0 ? (
            <div style={{ border: "1px solid var(--bd)", borderRadius: 13, background: "var(--panel)", padding: "32px 22px", textAlign: "center" }}>
              <div style={{ fontSize: 15, fontWeight: 700 }}>{q.trim() ? "No matches" : "Nothing published yet"}</div>
              <div style={{ fontSize: 13.5, color: "var(--fg2)", marginTop: 8, lineHeight: 1.6, maxWidth: 460, margin: "8px auto 0" }}>{q.trim() ? "No community skills match your search." : "Be the first — open a skill in My skills and click Publish to share it."}</div>
            </div>
          ) : (
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14 }}>
              {shownCommunity.map((e, i) => <CommunityCard key={i} e={e} installed={installed.has(e.name.toLowerCase())} installedVersion={installedVer[e.id]} onInstall={setReview} />)}
            </div>
          )
        )}
      </div>
      {form && <SkillForm initial={form.initial} onSave={saveSkill} onClose={() => setForm(null)} />}
      {review && <ReviewModal entry={review} onClose={() => setReview(null)} onInstalled={(name) => { setReview(null); setMsg(`Installed “${name}”.`); load(); invoke<Record<string, number>>("registry_installed").then(setInstalledVer).catch(() => {}); }} />}
    </Scroll>
  );
}

// ── Settings ─────────────────────────────────────────────────────────────────


/* Mission E2 — tiny zero-dependency markdown renderer for skill docs: headings,
   bold/italic/inline code, fenced code, lists, paragraphs. Links render as text
   (no navigation from untrusted docs — deliberate). */
export function MdView({ text }: { text: string }) {
  const blocks: React.ReactNode[] = [];
  const lines = text.split("\n");
  let i = 0, key = 0;
  const inline = (t: string): React.ReactNode[] => {
    const parts: React.ReactNode[] = [];
    let rest = t;
    const re = /(\*\*[^*]+\*\*|\*[^*]+\*|`[^`]+`|\[[^\]]+\]\([^)]+\))/;
    while (rest) {
      const m = rest.match(re);
      if (!m || m.index === undefined) { parts.push(rest); break; }
      if (m.index > 0) parts.push(rest.slice(0, m.index));
      const tok = m[0];
      if (tok.startsWith("**")) parts.push(<b key={key++}>{tok.slice(2, -2)}</b>);
      else if (tok.startsWith("`")) parts.push(<code key={key++} className="mono" style={{ fontSize: "0.92em", background: "var(--panel2)", padding: "1px 5px", borderRadius: 4 }}>{tok.slice(1, -1)}</code>);
      else if (tok.startsWith("[")) parts.push(<span key={key++}>{tok.slice(1, tok.indexOf("]"))} <span className="mono" style={{ fontSize: "0.85em", color: "var(--fg3)" }}>({tok.slice(tok.indexOf("(") + 1, -1)})</span></span>);
      else parts.push(<i key={key++}>{tok.slice(1, -1)}</i>);
      rest = rest.slice(m.index + tok.length);
    }
    return parts;
  };
  while (i < lines.length) {
    const l = lines[i];
    if (l.startsWith("```")) {
      const buf: string[] = []; i++;
      while (i < lines.length && !lines[i].startsWith("```")) { buf.push(lines[i]); i++; }
      i++;
      blocks.push(<pre key={key++} className="mono" style={{ background: "var(--panel2)", border: "1px solid var(--bd2)", borderRadius: 8, padding: "9px 11px", fontSize: 11.5, lineHeight: 1.55, overflow: "auto", margin: 0 }}>{buf.join("\n")}</pre>);
      continue;
    }
    if (/^#{1,3} /.test(l)) {
      const level = l.match(/^#+/)![0].length;
      blocks.push(<div key={key++} style={{ fontWeight: 700, fontSize: level === 1 ? 15 : level === 2 ? 13.5 : 12.5, marginTop: 6 }}>{inline(l.replace(/^#+ /, ""))}</div>);
      i++; continue;
    }
    if (/^[-*] /.test(l)) {
      const items: string[] = [];
      while (i < lines.length && /^[-*] /.test(lines[i])) { items.push(lines[i].slice(2)); i++; }
      blocks.push(<ul key={key++} style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 3 }}>{items.map((it, j) => <li key={j}>{inline(it)}</li>)}</ul>);
      continue;
    }
    if (l.trim() === "") { i++; continue; }
    const buf: string[] = [];
    while (i < lines.length && lines[i].trim() !== "" && !/^#{1,3} |^[-*] |^```/.test(lines[i])) { buf.push(lines[i]); i++; }
    blocks.push(<p key={key++} style={{ margin: 0, lineHeight: 1.55 }}>{inline(buf.join(" "))}</p>);
  }
  return <div style={{ display: "flex", flexDirection: "column", gap: 8, fontSize: 12.5, color: "var(--fg2)" }}>{blocks}</div>;
}
