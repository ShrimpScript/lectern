/* Brain — the memory/skills/codebase graph view. Extracted from App.tsx and
   lazy-loaded: the SVG graph + learn flow are never on the startup path, so
   they stay out of the boot chunk. */
import { useEffect, useRef, useState } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import { Icon, Scroll, WARN, DANGER, pushEv, type BrainGraphT, type Ev } from "./App";

// Brain-graph node hues (literals — used in SVG attributes, where CSS vars don't resolve).
const SKILL_HUE = "#9b8ec9"; // calm violet for skills (not green)
const BRAIN_COLORS: Record<string, string> = { root: "#8c8c86", skill: SKILL_HUE, trigger: "#66665f", memory: "#6f9ad6", session: WARN };
// App-wide in-flight task tracking. Long actions (e.g. building the code graph) run on the
// backend regardless of which page is shown; keeping their state in a module-level store —
// not in a component — means switching pages doesn't lose track of them. The action keeps
// running and the UI re-syncs when you come back, instead of looking cancelled.
const buildingPaths = new Set<string>();
const buildErrors = new Map<string, string>();
const taskSubs = new Set<() => void>();
const notifyTasks = () => taskSubs.forEach((f) => f());
// Subscribe a component to task-state changes so it re-renders when a build starts/finishes.
function useTaskState() {
  const [, force] = useState(0);
  useEffect(() => {
    const f = () => force((n) => n + 1);
    taskSubs.add(f);
    return () => { taskSubs.delete(f); };
  }, []);
}
async function startBuildGraph(path: string): Promise<void> {
  if (!path.trim() || buildingPaths.has(path)) return;
  buildingPaths.add(path);
  buildErrors.delete(path);
  notifyTasks();
  try {
    await invoke<string>("build_code_graph", { path });
  } catch (e) {
    buildErrors.set(path, String(e));
  }
  buildingPaths.delete(path);
  notifyTasks();
}

export function Brain({ path }: { path: string }) {
  const [g, setG] = useState<BrainGraphT | null>(null);
  const [pos, setPos] = useState<Record<string, { x: number; y: number }>>({});
  const [hover, setHover] = useState<string | null>(null);
  // System profile: always-on machine knowledge injected into every session.
  const [sys, setSys] = useState<{ learned: boolean; age_days: number | null; preview: string } | null>(null);
  const [learning, setLearning] = useState(false);
  const [learnMsg, setLearnMsg] = useState("");
  const [showProfile, setShowProfile] = useState(false);
  const autoRan = useRef(false);
  const refreshSys = () => invoke<{ learned: boolean; age_days: number | null; preview: string }>("system_profile_status").then(setSys).catch(() => {});
  // Graphify code graph for this workspace (Phase B).
  const [code, setCode] = useState<{ built: boolean; nodes: number; edges: number; communities: number; top: string[] } | null>(null);
  const refreshCode = () => { if (!path.trim()) { setCode(null); return; } invoke<{ built: boolean; nodes: number; edges: number; communities: number; top: string[] }>("code_graph", { path }).then(setCode).catch(() => setCode(null)); };
  useEffect(refreshCode, [path]);
  // The build runs app-wide (survives switching pages) — its state lives in the task store.
  useTaskState();
  const building = buildingPaths.has(path);
  const buildMsg = buildErrors.get(path) ?? "";
  const buildGraph = () => startBuildGraph(path);
  // When a build for this folder finishes (even while we were on another page), re-fetch.
  const wasBuilding = useRef(building);
  useEffect(() => { if (wasBuilding.current && !building) refreshCode(); wasBuilding.current = building; }, [building]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { refreshSys(); }, []);
  const learnSystem = () => {
    if (learning) return;
    setLearning(true); setLearnMsg("Starting the scan…");
    const ch = new Channel<Ev>();
    ch.onmessage = (ev) => { if (ev.type === "message") setLearnMsg(String((ev as { text: string }).text).replace(/^🧠\s*/, "").slice(0, 160)); };
    invoke("learn_system_session", { sessionId: "system-learn", onEvent: ch })
      .then(() => { setLearning(false); setLearnMsg(""); refreshSys(); })
      .catch((e) => { setLearning(false); setLearnMsg(`Couldn't learn the system: ${String(e)}`); });
  };
  // Periodic refresh: if a profile exists but is stale (>7 days), re-learn once on open.
  useEffect(() => {
    if (sys?.learned && (sys.age_days ?? 0) > 7 && !autoRan.current && !learning) {
      autoRan.current = true;
      learnSystem();
    }
  }, [sys, learning]);
  const [gState, setGState] = useState<"loading" | "ready" | "no-path" | "error">("loading");
  useEffect(() => {
    if (!path.trim()) { setG(null); setGState("no-path"); return; }
    setGState("loading");
    invoke<BrainGraphT>("brain_graph", { path })
      .then((v) => { setG(v); setGState("ready"); })
      .catch(() => { setG(null); setGState("error"); });
  }, [path]);
  useEffect(() => {
    if (!g || g.nodes.length === 0) { setPos({}); return; }
    const W = 1000, H = 680, cx = W / 2, cy = H / 2;
    const p: Record<string, { x: number; y: number; vx: number; vy: number }> = {};
    g.nodes.forEach((n, i) => {
      const a = (i / g.nodes.length) * Math.PI * 2;
      p[n.id] = n.kind === "root" ? { x: cx, y: cy, vx: 0, vy: 0 } : { x: cx + Math.cos(a) * 220, y: cy + Math.sin(a) * 220, vx: 0, vy: 0 };
    });
    for (let it = 0; it < 320; it++) {
      for (let i = 0; i < g.nodes.length; i++) for (let j = i + 1; j < g.nodes.length; j++) {
        const a = p[g.nodes[i].id], b = p[g.nodes[j].id];
        const dx = a.x - b.x, dy = a.y - b.y, d2 = dx * dx + dy * dy + 0.01, d = Math.sqrt(d2);
        const rep = 2400 / d2, fx = (dx / d) * rep, fy = (dy / d) * rep;
        a.vx += fx; a.vy += fy; b.vx -= fx; b.vy -= fy;
      }
      for (const e of g.edges) {
        const a = p[e.from], b = p[e.to]; if (!a || !b) continue;
        const dx = b.x - a.x, dy = b.y - a.y, d = Math.sqrt(dx * dx + dy * dy) + 0.01;
        const k = (d - 96) * 0.012, fx = (dx / d) * k, fy = (dy / d) * k;
        a.vx += fx; a.vy += fy; b.vx -= fx; b.vy -= fy;
      }
      for (const n of g.nodes) {
        const a = p[n.id];
        if (n.kind === "root") { a.x = cx; a.y = cy; a.vx = 0; a.vy = 0; continue; }
        a.vx += (cx - a.x) * 0.0016; a.vy += (cy - a.y) * 0.0016;
        a.vx *= 0.86; a.vy *= 0.86; a.x += a.vx; a.y += a.vy;
      }
    }
    const out: Record<string, { x: number; y: number }> = {};
    for (const n of g.nodes) out[n.id] = { x: p[n.id].x, y: p[n.id].y };
    setPos(out);
  }, [g]);

  const hasGraph = g && g.nodes.length > 1 && Object.keys(pos).length > 0;
  return (
    <Scroll>
      <div style={{ maxWidth: 1040, margin: "0 auto", padding: "44px 40px", display: "flex", flexDirection: "column", gap: 18 }}>
        <div>
          <div style={{ fontSize: 26, fontWeight: 800, letterSpacing: "-0.02em" }}>Brain</div>
          <div style={{ fontSize: 14, color: "var(--fg3)", marginTop: 4 }}>What Lectern knows about this workspace.</div>
        </div>
        {!path.trim() ? (
          <div className="mono" style={{ fontSize: 13, color: WARN, border: "1px solid var(--bd)", background: "var(--panel)", borderRadius: 10, padding: "12px 14px" }}>Open a project in Chat first — the brain is per-workspace.</div>
        ) : (
          <>
            <div style={{ display: "flex", gap: 18, flexWrap: "wrap" }}>
              {([["skills", g?.skills ?? 0, SKILL_HUE], ["memory files", g?.memory ?? 0, "#6f9ad6"], ["sessions", g?.sessions ?? 0, WARN]] as const).map(([label, n, c]) => (
                <div key={label} style={{ display: "flex", alignItems: "center", gap: 7, fontSize: 13, color: "var(--fg2)" }}><span style={{ width: 9, height: 9, borderRadius: "50%", background: c }} /><b style={{ color: "var(--fg)" }}>{n}</b> {label}</div>
              ))}
            </div>
            {/* System profile — always-on machine knowledge for every session */}
            <div style={{ border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", padding: "14px 16px", display: "flex", flexDirection: "column", gap: 9 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 11 }}>
                <span style={{ display: "flex", color: sys?.learned ? "var(--fg)" : "var(--fg3)" }}><Icon name="brain" size={16} /></span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontWeight: 700, fontSize: 14 }}>System profile</div>
                  <div style={{ fontSize: 12, color: "var(--fg3)", lineHeight: 1.45 }}>
                    {learning
                      ? (learnMsg || "Learning your machine…")
                      : sys?.learned
                        ? `Learned ${sys.age_days === 0 || sys.age_days == null ? "today" : `${sys.age_days}d ago`} — injected into every session so agents already know your machine.`
                        : "Teach Lectern about your machine (OS, tools, config) so every agent starts with the right context."}
                  </div>
                </div>
                {learning ? (
                  <span className="mono" style={{ fontSize: 12, color: "var(--fg3)", flexShrink: 0 }}>Working…</span>
                ) : (
                  <button onClick={learnSystem} style={{ flexShrink: 0, height: 32, padding: "0 14px", borderRadius: 9, border: sys?.learned ? "1px solid var(--bd)" : "none", background: sys?.learned ? "transparent" : "var(--btn)", color: sys?.learned ? "var(--fg)" : "var(--btnfg)", fontSize: 13, fontWeight: 700, cursor: "pointer", fontFamily: "inherit" }}>{sys?.learned ? "Refresh" : "Learn my system"}</button>
                )}
              </div>
              {!learning && sys?.learned && sys.preview && (
                <>
                  <button onClick={() => setShowProfile((v) => !v)} className="mono" style={{ alignSelf: "flex-start", border: "none", background: "transparent", color: "var(--fg3)", fontSize: 11, cursor: "pointer", padding: 0 }}>{showProfile ? "▾ hide profile" : "▸ view profile"}</button>
                  {showProfile && <div className="mono" style={{ fontSize: 11, color: "var(--fg2)", background: "var(--elev)", border: "1px solid var(--bd2)", borderRadius: 8, padding: "10px 12px", maxHeight: 280, overflow: "auto", whiteSpace: "pre-wrap" }}>{sys.preview}</div>}
                </>
              )}
            </div>
            {/* Code graph (graphify) — Phase B: structure surfaced + fed into recall */}
            <div style={{ border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", padding: "14px 16px", display: "flex", flexDirection: "column", gap: 9 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 11 }}>
                <span style={{ color: code?.built ? "var(--fg)" : "var(--fg3)", display: "flex" }}><Icon name="branch" size={16} /></span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontWeight: 700, fontSize: 14 }}>Code graph</div>
                  <div style={{ fontSize: 12, color: "var(--fg3)", lineHeight: 1.45 }}>
                    {building ? "Building the code graph (graphify update)…"
                      : code?.built
                        ? `${code.nodes.toLocaleString()} symbols · ${code.edges.toLocaleString()} links · ${code.communities} clusters — fed into recall so agents start knowing the code structure.`
                        : "Not built yet — map this repo’s symbols + dependencies so agents (and recall) know its structure."}
                  </div>
                </div>
                {path.trim() && (
                  <button onClick={buildGraph} disabled={building} style={{ flexShrink: 0, height: 32, padding: "0 13px", borderRadius: 9, border: code?.built ? "1px solid var(--bd)" : "none", background: code?.built ? "transparent" : "var(--btn)", color: code?.built ? "var(--fg)" : "var(--btnfg)", fontSize: 12.5, fontWeight: 700, cursor: building ? "default" : "pointer", fontFamily: "inherit", opacity: building ? 0.6 : 1 }}>{building ? "Building…" : code?.built ? "Rebuild" : "Build code graph"}</button>
                )}
              </div>
              {buildMsg && <div className="mono" style={{ fontSize: 11, color: DANGER, lineHeight: 1.5 }}>{buildMsg}</div>}
              {code?.built && code.top.length > 0 && (
                <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                  {code.top.map((t) => <span key={t} className="mono" style={{ fontSize: 11, color: "var(--fg2)", background: "var(--elev)", border: "1px solid var(--bd2)", borderRadius: 6, padding: "2px 8px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 160 }}>{t}</span>)}
                </div>
              )}
            </div>
            <div style={{ border: "1px solid var(--bd)", borderRadius: 14, background: "var(--tree)", overflow: "hidden" }}>
              {hasGraph ? (
                <svg viewBox="0 0 1000 680" style={{ width: "100%", height: "auto", display: "block" }}>
                  {g!.edges.map((e, i) => {
                    const a = pos[e.from], b = pos[e.to]; if (!a || !b) return null;
                    const hot = !!hover && (e.from === hover || e.to === hover);
                    return <line key={i} x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke={hot ? SKILL_HUE : "#3a3a40"} strokeWidth={hot ? 1.4 : 0.7} opacity={hover && !hot ? 0.2 : 0.65} />;
                  })}
                  {g!.nodes.map((n) => {
                    const a = pos[n.id]; if (!a) return null;
                    const r = n.kind === "root" ? 13 : Math.max(4, Math.min(11, 4 + n.weight * 2));
                    const c = BRAIN_COLORS[n.kind] ?? "var(--fg2)";
                    const showLabel = n.kind === "root" || n.kind === "skill" || n.kind === "session" || hover === n.id;
                    return (
                      <g key={n.id} onMouseEnter={() => setHover(n.id)} onMouseLeave={() => setHover((h) => (h === n.id ? null : h))}>
                        <circle cx={a.x} cy={a.y} r={r} fill={c} opacity={hover && hover !== n.id ? 0.45 : 1} strokeWidth={1.5} style={{ stroke: "var(--bg)" }} />
                        {showLabel && <text x={a.x + r + 3} y={a.y + 3.5} fontSize={n.kind === "root" ? 13 : 10.5} fontWeight={n.kind === "root" || n.kind === "skill" ? 600 : 400} fill="var(--fg2)" style={{ pointerEvents: "none", fontFamily: "Archivo, sans-serif" }}>{n.label.length > 22 ? n.label.slice(0, 21) + "…" : n.label}</text>}
                      </g>
                    );
                  })}
                </svg>
              ) : (
                <div className="mono" style={{ fontSize: 12.5, color: "var(--fg3)", padding: 40, textAlign: "center" }}>
                  {gState === "no-path"
                    ? "This chat isn't tied to a project folder yet — open a chat in a folder and its brain grows here."
                    : gState === "error"
                      ? "Couldn't read the brain for this folder — is it still on disk?"
                      : gState === "loading"
                        ? "loading…"
                        : "No memory yet — run a session or index this repo to grow the brain."}
                </div>
              )}
            </div>
            <div style={{ display: "flex", gap: 16, flexWrap: "wrap", fontSize: 11.5, color: "var(--fg3)" }}>
              {([["Workspace", "#8c8c86"], ["Skill", SKILL_HUE], ["Trigger", "#66665f"], ["Memory file", "#6f9ad6"], ["Session", WARN]] as const).map(([l, c]) => (
                <span key={l} style={{ display: "flex", alignItems: "center", gap: 6 }}><span style={{ width: 8, height: 8, borderRadius: "50%", background: c }} />{l}</span>
              ))}
            </div>
          </>
        )}
      </div>
    </Scroll>
  );
}

// ── Schedule ─────────────────────────────────────────────────────────────────
