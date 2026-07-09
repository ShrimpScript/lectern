/* Settings — providers, connections (MCP), defaults, appearance, routing.
   Extracted from App.tsx and lazy-loaded (never on the startup path). */
import { useEffect, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CatalogRow, MCP_CATALOG } from "./Connect";
import { mcpIconFor, providerIcon } from "./BrandIcons";
import { ACCENT, ctrl, DANGER, WARN, Icon, Scroll, Section, THEME_VAR_WHITELIST, type BackendInfo, type McpServer, type ModelOpt, type Prefs, type ThemeName, NiceSelect, Switch, SunIcon, MoonIcon } from "./App";

// ── Connections (MCP) — Settings card (moved out of the Personal Agent rail) ─

/* What each provider actually gives you + how to get it. */
const PROVIDER_UNLOCKS: Record<string, string> = {
  "claude-code": "Claude models — Fable, Opus, Sonnet, Haiku (uses your Claude subscription)",
  antigravity: "Gemini models — Flash and Pro (uses your Google account)",
  opencode: "OpenRouter + ~75 providers; built-in free models work with no API key at all",
};
const PROVIDER_INSTALL: Record<string, string> = {
  "claude-code": "npm i -g @anthropic-ai/claude-code · then run claude once",
  antigravity: "install Antigravity · then run agy once",
  opencode: "curl -fsSL https://opencode.ai/install | bash",
};

type OS = "macos" | "linux" | "windows";
type SetupSpec = {
  cmd: Partial<Record<OS, string>>; // install command per OS
  oneClick?: boolean; // has a vetted run_setup installer (unix)
  auth?: string; // step after install
  guide: string; // Lectern docs (allowed by open_url)
  external?: string; // provider's official page
};
// Everything the setup helper needs, per provider. Commands mirror the vetted ones in
// the `run_setup` Tauri command so one-click and copy-paste stay in sync.
const PROVIDER_SETUP: Record<string, SetupSpec> = {
  "claude-code": {
    cmd: { macos: "npm i -g @anthropic-ai/claude-code", linux: "npm i -g @anthropic-ai/claude-code", windows: "npm i -g @anthropic-ai/claude-code" },
    auth: "then run `claude` once and sign in with your Claude subscription.",
    guide: "https://getlectern.vercel.app/docs/integrations",
    external: "https://docs.claude.com/claude-code",
  },
  antigravity: {
    cmd: { macos: "Download Antigravity, then run `agy` once", linux: "Download Antigravity, then run `agy` once", windows: "Download Antigravity, then run `agy` once" },
    auth: "signs in with your Google account; no API key.",
    guide: "https://getlectern.vercel.app/docs/integrations",
    external: "https://antigravity.google/",
  },
  opencode: {
    cmd: { macos: "curl -fsSL https://opencode.ai/install | bash", linux: "curl -fsSL https://opencode.ai/install | bash", windows: "See the guide (npm or scoop)" },
    oneClick: true,
    auth: "built-in free models work with no key; for OpenRouter/others run `opencode auth login`.",
    guide: "https://getlectern.vercel.app/docs/integrations",
    external: "https://opencode.ai/",
  },
  openrouter: {
    cmd: { macos: "Included with OpenCode — run `opencode auth login`", linux: "Included with OpenCode — run `opencode auth login`", windows: "Included with OpenCode — run `opencode auth login`" },
    auth: "add your OpenRouter key when prompted; then its models appear in the picker.",
    guide: "https://getlectern.vercel.app/docs/integrations",
    external: "https://openrouter.ai/keys",
  },
  ollama: {
    cmd: { linux: "curl -fsSL https://ollama.com/install.sh | sh", macos: "brew install ollama", windows: "Download the Windows installer" },
    oneClick: true,
    auth: "then pull a code model — `ollama pull qwen3-coder` — Lectern auto-detects it (the picker flags code-strong models).",
    guide: "https://getlectern.vercel.app/docs/integrations",
    external: "https://ollama.com/",
  },
};

function CopyBtn({ text }: { text: string }) {
  const [ok, setOk] = useState(false);
  return (
    <button onClick={() => { navigator.clipboard?.writeText(text).then(() => { setOk(true); setTimeout(() => setOk(false), 1400); }).catch(() => {}); }}
      style={{ height: 26, padding: "0 10px", fontSize: 11.5, fontWeight: 600, color: "var(--fg2)", background: "transparent", border: "1px solid var(--bd)", borderRadius: 7, cursor: "pointer", fontFamily: "inherit", flexShrink: 0 }}>
      {ok ? "Copied ✓" : "Copy"}
    </button>
  );
}

// Expandable per-provider setup: OS-aware command + one-click install (where vetted)
// + copy + guide/official links. Run the install here when it's safe, and always
// leave the copy-paste command + guide as the fallback.
function ProviderSetup({ id, os, onRecheck }: { id: string; os: OS; onRecheck: () => void }) {
  const spec = PROVIDER_SETUP[id];
  const [busy, setBusy] = useState(false);
  const [out, setOut] = useState("");
  if (!spec) return null;
  const cmd = spec.cmd[os] ?? spec.cmd.linux ?? "See the guide";
  const runnable = !!spec.oneClick && os !== "windows";
  const install = () => {
    setBusy(true); setOut("Running…");
    invoke<string>("run_setup", { provider: id })
      .then((r) => { setOut(r); onRecheck(); })
      .catch((e) => setOut(String(e)))
      .finally(() => setBusy(false));
  };
  return (
    <div style={{ marginTop: 10, padding: "12px 14px", background: "var(--panel2)", border: "1px solid var(--bd2)", borderRadius: 10, display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <code style={{ flex: 1, minWidth: 180, fontSize: 11.5, color: "var(--fg)", background: "var(--bg)", border: "1px solid var(--bd)", borderRadius: 7, padding: "6px 9px", overflowX: "auto", whiteSpace: "nowrap" }} className="mono">{cmd}</code>
        <CopyBtn text={cmd} />
        {runnable && (
          <button onClick={install} disabled={busy}
            style={{ height: 26, padding: "0 12px", fontSize: 11.5, fontWeight: 700, color: busy ? "var(--fg3)" : "var(--btnfg)", background: busy ? "transparent" : "var(--btn)", border: busy ? "1px solid var(--bd)" : "none", borderRadius: 7, cursor: busy ? "default" : "pointer", fontFamily: "inherit", flexShrink: 0 }}>
            {busy ? "Installing…" : "Install"}
          </button>
        )}
      </div>
      {spec.auth && <div style={{ fontSize: 11.5, color: "var(--fg2)", lineHeight: 1.5 }}>{spec.auth}</div>}
      {out && <pre style={{ margin: 0, maxHeight: 160, overflow: "auto", fontSize: 10.5, color: "var(--fg2)", background: "var(--bg)", border: "1px solid var(--bd)", borderRadius: 7, padding: "8px 10px", whiteSpace: "pre-wrap" }} className="mono">{out}</pre>}
      <div style={{ display: "flex", gap: 14, fontSize: 11.5 }}>
        <button onClick={() => invoke("open_url", { url: spec.guide }).catch(() => {})} style={{ background: "none", border: "none", color: "var(--fg)", cursor: "pointer", fontFamily: "inherit", fontSize: 11.5, textDecoration: "underline", padding: 0 }}>Full guide →</button>
        {spec.external && <button onClick={() => invoke("open_url", { url: spec.external! }).catch(() => {})} style={{ background: "none", border: "none", color: "var(--fg2)", cursor: "pointer", fontFamily: "inherit", fontSize: 11.5, padding: 0 }}>Official page →</button>}
      </div>
    </div>
  );
}

function ModelPicker({ backend, model, backends, models, onChange }: { backend: string; model: string; backends: BackendInfo[]; models: ModelOpt[]; onChange: (backend: string, model: string) => void }) {
  const avail = (b: string) => b === "auto" || b === "mock" || (backends.find((x) => x.id === b)?.available ?? false);
  const cur = models.find((m) => m.backend === backend && (m.backend === "auto" || m.backend === "mock" || m.model === model)) ?? models[0];
  return (
    <NiceSelect
      value={cur?.id ?? ""}
      title="Model for this session"
      items={models.map((m) => ({ id: m.id, label: m.label, group: m.group || undefined, disabled: !avail(m.backend), hint: avail(m.backend) ? undefined : "not connected" }))}
      onPick={(id) => { const o = models.find((m) => m.id === id); if (o) onChange(o.backend, o.model); }}
    />
  );
}

// Render the draft with /command tokens highlighted as pills. Background-only (no padding
// or border) so the highlight layer stays pixel-aligned with the transparent textarea above it.

export function Settings({ backends, models, prefs, mcp, onMcp, onPrefs, onRecheck, onBrowse }: { backends: BackendInfo[]; models: ModelOpt[]; prefs: Prefs; mcp: McpServer[]; onMcp: () => void; onPrefs: (p: Partial<Prefs>) => void; onRecheck: () => void; onBrowse?: () => void }) {
  const theme = prefs.theme;
  const [classifier, setClassifier] = useState(false);
  const [os, setOs] = useState<OS>("linux");
  const [setupOpen, setSetupOpen] = useState<string | null>(null);
  useEffect(() => { invoke<OS>("os_platform").then(setOs).catch(() => {}); }, []);
  useEffect(() => { invoke<boolean>("routing_classifier").then(setClassifier).catch(() => {}); }, []);
  const toggleClassifier = () => { const v = !classifier; setClassifier(v); invoke("set_routing_classifier", { on: v }).catch(() => {}); };
  const themeToggle = (
    <div role="radiogroup" aria-label="Theme" style={{ display: "flex", gap: 3, background: "var(--panel2)", border: "1px solid var(--bd)", borderRadius: 9, padding: 3, fontSize: 13, fontWeight: 600 }}>
      {(["light", "dark"] as ThemeName[]).map((t) => (
        <button key={t} role="radio" aria-checked={theme === t} onClick={() => onPrefs({ theme: t })}
          style={{ height: 30, padding: "0 14px", display: "inline-flex", alignItems: "center", gap: 7, border: "none", fontFamily: "inherit", fontSize: 13, fontWeight: 600, justifyContent: "center", borderRadius: 6, cursor: "pointer", textTransform: "capitalize", background: theme === t ? "var(--btn)" : "transparent", color: theme === t ? "var(--btnfg)" : "var(--fg2)", transition: "background .18s ease, color .18s ease" }}>
          {t === "light" ? <SunIcon /> : <MoonIcon />}{t}
        </button>
      ))}
    </div>
  );
  return (
    <Scroll>
      <div style={{ maxWidth: 680, margin: "0 auto", padding: "44px 40px", display: "flex", flexDirection: "column", gap: 30 }}>
        <div style={{ fontSize: 26, fontWeight: 800, letterSpacing: "-0.02em" }}>Settings</div>

        <Section label="Providers & models">
          <div style={{ fontSize: 13, color: "var(--fg2)", lineHeight: 1.6, marginBottom: 10 }}>
            Lectern drives agent tools installed on this machine — <b style={{ color: "var(--fg)" }}>Claude Code</b>, <b style={{ color: "var(--fg)" }}>Antigravity</b>, and <b style={{ color: "var(--fg)" }}>OpenCode</b>. Each connected provider unlocks its models in the model menu; pick one per session or let <b style={{ color: "var(--fg)" }}>Auto</b> route each task. The Conductor (<span className="mono">/conduct</span>) hands sub-tasks to whichever model fits.
          </div>
          <div style={{ border: "1px solid var(--bd)", borderRadius: 12, overflow: "hidden", background: "var(--panel)" }}>
            {backends.filter((b) => b.id !== "mock").map((b, i) => {
              const hasSetup = !!PROVIDER_SETUP[b.id];
              const open = setupOpen === b.id;
              return (
              <div key={b.id} style={{ borderTop: i ? "1px solid var(--bd2)" : "none" }}>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 12, padding: "14px 16px" }}>
                  <div style={{ display: "flex", alignItems: "flex-start", gap: 12, minWidth: 0 }}>
                    {providerIcon(b.id, 17) && (
                      <span style={{ width: 32, height: 32, border: "1px solid var(--bd)", borderRadius: 8, display: "inline-flex", alignItems: "center", justifyContent: "center", color: "var(--fg)", flexShrink: 0 }}>{providerIcon(b.id, 17)}</span>
                    )}
                    <div style={{ minWidth: 0, lineHeight: 1.45 }}>
                      <div style={{ fontWeight: 600, fontSize: 14 }}>{b.label.replace(" (many providers)", "")}</div>
                      <div style={{ fontSize: 12, color: "var(--fg2)", marginTop: 2 }}>{PROVIDER_UNLOCKS[b.id] ?? ""}</div>
                      <div style={{ fontSize: 11.5, color: "var(--fg3)", marginTop: 2 }}>
                        {b.available
                          ? <span className="mono" style={{ fontSize: 10.5 }}>{b.detail}</span>
                          : <>Not installed{hasSetup ? "" : <> — <span className="mono" style={{ fontSize: 10.5 }}>{PROVIDER_INSTALL[b.id] ?? "see docs"}</span></>}</>}
                      </div>
                    </div>
                  </div>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, flexShrink: 0 }}>
                    {hasSetup && (
                      <button onClick={() => setSetupOpen(open ? null : b.id)}
                        style={{ height: 26, padding: "0 11px", fontSize: 11.5, fontWeight: 600, color: "var(--fg2)", background: "transparent", border: "1px solid var(--bd)", borderRadius: 7, cursor: "pointer", fontFamily: "inherit" }}>
                        {open ? "Close" : b.available ? "Set up ▾" : "Install ▾"}
                      </button>
                    )}
                    <span style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 11.5, fontWeight: 600, color: b.available ? "var(--fg)" : "var(--fg3)", border: "1px solid var(--bd)", borderRadius: 999, padding: "4px 11px" }}>
                      <span style={{ width: 6, height: 6, borderRadius: "50%", background: b.available ? ACCENT : "var(--bd2)" }} />
                      {b.available ? "Connected" : "Not installed"}
                    </span>
                  </div>
                </div>
                {open && <div style={{ padding: "0 16px 14px 60px" }}><ProviderSetup id={b.id} os={os} onRecheck={onRecheck} /></div>}
              </div>
              );
            })}
          </div>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 14 }}>
            <div className="mono" style={{ fontSize: 11, color: "var(--fg3)", lineHeight: 1.6, flex: 1 }}>Managed locally — keys stay on your machine, never our servers. Add/re-auth via the CLI (`claude`, `lectern login`).</div>
            <button onClick={onRecheck} style={{ height: 30, flexShrink: 0, padding: "0 12px", display: "inline-flex", alignItems: "center", gap: 6, fontSize: 12, color: "var(--fg)", background: "transparent", border: "1px solid var(--bd)", borderRadius: 8, cursor: "pointer", fontFamily: "inherit" }}>↻ Re-check</button>
          </div>
        </Section>

        <Section label="Tools (MCP)">
          <div style={{ fontSize: 13, color: "var(--fg2)", lineHeight: 1.6, marginBottom: 10 }}>
            MCP servers give every run extra tools — files, APIs, browsers, other services. They&apos;re
            registered with Claude Code (<span className="mono">claude mcp</span>), so they work across
            chats, the Personal Agent, and the Conductor. Add one with a command or an https URL.
          </div>
          <div style={{ border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", padding: "14px 16px" }}>
            <ConnectionsCard mcp={mcp} onRefresh={onMcp} backends={backends} onBrowse={onBrowse} />
          </div>
        </Section>

        <Section label="Remote access">
          <RemoteAccessCard onBrowse={onBrowse} />
        </Section>

        <Section label="Session defaults">
          <div style={{ border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", padding: "4px 16px" }}>
            <PrefRow label="Default model" desc="The model new sessions start with. “Auto” lets Lectern route each task to the best model.">
              <ModelPicker backend={prefs.default_backend} model={prefs.default_model} backends={backends} models={models} onChange={(b, m) => onPrefs({ default_backend: b, default_model: m })} />
            </PrefRow>
            <PrefRow label="Apply edits by default" desc="Start new sessions in apply mode (edits land) instead of plan.">
              <Switch on={prefs.default_apply} label={prefs.default_apply ? "On" : "Off"} onClick={() => onPrefs({ default_apply: !prefs.default_apply })} />
            </PrefRow>
            <PrefRow label="Clean output by default" desc="New chats start in Clean view — machinery folded into one strip. Each chat can still flip its own toggle." last>
              <Switch on={prefs.clean_output} label={prefs.clean_output ? "On" : "Off"} onClick={() => onPrefs({ clean_output: !prefs.clean_output })} />
            </PrefRow>
          </div>
        </Section>

        <Section label="Automatic fallback">
          <div style={{ border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", padding: "15px 16px" }}>
            <div style={{ fontSize: 13, color: "var(--fg2)", lineHeight: 1.55 }}>If a backend hits its usage limit mid-session, Lectern continues on the next available one. Set a per-run fallback with <span className="mono" style={{ color: "var(--fg)" }}>--fallback-model</span>, or auto-continue on a schedule after a limit.</div>
          </div>
        </Section>

        <Section label="About you">
          <AboutYouCard />
        </Section>

        <Section label="About Lectern">
          <AboutLecternCard />
        </Section>

        <Section label="Appearance">
          <div style={{ border: "1px solid var(--bd)", borderRadius: 12, padding: "16px 18px", background: "var(--panel)", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
            <div><div style={{ fontWeight: 600, fontSize: 15 }}>Theme</div><div style={{ fontSize: 13, color: "var(--fg2)", marginTop: 3 }}>Light is the default. Dark is fully supported.</div></div>
            {themeToggle}
          </div>
          <ThemeManagerCard prefs={prefs} onPrefs={onPrefs} />
        </Section>

        <Section label="Routing">
          <div style={{ border: "1px solid var(--bd)", borderRadius: 12, padding: "16px 18px", background: "var(--panel)", display: "flex", justifyContent: "space-between", alignItems: "center", gap: 16 }}>
            <div><div style={{ fontWeight: 600, fontSize: 15 }}>Smart routing (classifier)</div><div style={{ fontSize: 13, color: "var(--fg2)", marginTop: 3, lineHeight: 1.5 }}>For tasks that match no preset rule, a fast model picks the best model. More accurate on ambiguous tasks; adds one quick call. Presets always apply first.</div></div>
            <Switch on={classifier} label={classifier ? "On" : "Off"} onClick={toggleClassifier} />
          </div>
          <RoutingConfigCard />
        </Section>

        <Section label="Help">
          <div style={{ border: "1px solid var(--bd)", borderRadius: 12, padding: "16px 18px", background: "var(--panel)", display: "flex", justifyContent: "space-between", alignItems: "center", gap: 16 }}>
            <div><div style={{ fontWeight: 600, fontSize: 15 }}>Replay onboarding</div><div style={{ fontSize: 13, color: "var(--fg2)", marginTop: 3 }}>Show the first-run setup checklist again.</div></div>
            <button onClick={() => onPrefs({ onboarded: false })} style={{ flexShrink: 0, height: 32, padding: "0 14px", borderRadius: 9, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg)", fontSize: 13, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>Replay</button>
          </div>
        </Section>
      </div>
    </Scroll>
  );
}

function PrefRow({ label, desc, children, last }: { label: string; desc: string; children: React.ReactNode; last?: boolean }) {
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 16, padding: "13px 0", borderBottom: last ? "none" : "1px solid var(--bd2)" }}>
      <div><div style={{ fontWeight: 600, fontSize: 14 }}>{label}</div><div style={{ fontSize: 12.5, color: "var(--fg2)", marginTop: 2, lineHeight: 1.5 }}>{desc}</div></div>
      <div style={{ flexShrink: 0 }}>{children}</div>
    </div>
  );
}

// Clean line icons (Omnigent/Claude style), stroke = currentColor.
// Clean, consistent line icons (Lucide) behind our stable name-based API.


function ConnectionsCard({ mcp, onRefresh, backends = [], onBrowse }: { mcp: McpServer[]; onRefresh?: () => void; backends?: BackendInfo[]; onBrowse?: () => void }) {
  const has = (id: string) => backends.find((b) => b.id === id)?.available ?? false;
  const detected = [has("claude-code") && "Claude Code", has("opencode") && "OpenCode", has("antigravity") && "Antigravity"].filter(Boolean) as string[];
  const [adding, setAdding] = useState(false);
  const [q, setQ] = useState("");
  const [showAll, setShowAll] = useState(false);
  const [name, setName] = useState("");
  const [cmd, setCmd] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const add = () => {
    if (!name.trim() || !cmd.trim()) return;
    invoke<Record<string, string>>("add_mcp", { name, command: cmd }).then((r) => { setName(""); setCmd(""); setAdding(false); const extras = ["opencode", "antigravity"].filter((h) => r?.[h] === "ok"); setMsg(extras.length ? `Added — also registered in ${extras.join(" + ")}.` : null); onRefresh?.(); }).catch((e) => setMsg(String(e)));
  };
  const remove = (n: string) => invoke("remove_mcp", { name: n }).then(() => onRefresh?.()).catch((e) => setMsg(String(e)));
  // Live status: re-list every 30s while Settings is open AND visible (each
  // poll spawns the claude CLI — at 10s with a hidden window that was a steady
  // background CPU burst users felt as lag spikes).
  useEffect(() => {
    const t = setInterval(() => {
      if (!document.hidden) onRefresh?.();
    }, 30_000);
    return () => clearInterval(t);
  }, [onRefresh]);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 9 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <div className="mono" style={{ fontSize: 11, fontWeight: 600, color: "var(--fg3)", letterSpacing: "0.06em" }}>INSTALLED</div>
      <div style={{ fontSize: 11.5, color: "var(--fg3)", lineHeight: 1.5, marginTop: -4 }}>Registered with Claude Code — every chat, the Personal Agent, and the Conductor can call these. <span style={{ color: ACCENT }}>●</span> responding now · <span style={{ color: DANGER }}>●</span> not responding (it retries on next use).</div>
        <span onClick={onRefresh} title="refresh" style={{ cursor: "pointer", color: "var(--fg3)", fontSize: 13 }}>⟳</span>
      </div>
      {(() => { const brain = mcp.find((m) => m.name === "lectern-brain"); return brain ? (
        <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "8px 10px", borderRadius: 9, border: "1px solid var(--bd)", background: "var(--panel)", color: "var(--fg2)", fontSize: 12 }}>
          <span title={brain.connected ? "connected" : "registered"} style={{ width: 6, height: 6, borderRadius: "50%", background: brain.connected ? ACCENT : WARN, flexShrink: 0 }} />
          <span style={{ flex: 1, fontWeight: 600 }}>Lectern brain</span>
          <span style={{ fontSize: 10.5, color: "var(--fg3)", flexShrink: 0 }}>{brain.connected ? "running" : "registered"}</span>
          <span onClick={() => remove("lectern-brain")} title="disconnect" style={{ cursor: "pointer", color: "var(--fg3)", fontSize: 15, lineHeight: 1 }}>×</span>
        </div>
      ) : (
        <button onClick={() => invoke("connect_brain").then(() => { setMsg(null); onRefresh?.(); }).catch((e) => setMsg(String(e)))}
          style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 10px", borderRadius: 9, border: "1px solid var(--bd)", background: "var(--panel)", color: "var(--fg2)", cursor: "pointer", fontFamily: "inherit", textAlign: "left" }}>
          <span style={{ lineHeight: 1.35 }}><div style={{ fontWeight: 700, fontSize: 12.5, color: "var(--fg)" }}>Connect Lectern's brain</div><div style={{ fontSize: 10.5, color: "var(--fg3)" }}>Query memory + skills via MCP</div></span>
        </button>
      ); })()}
      {(() => { const g = mcp.find((m) => m.name === "lectern-graphify"); return g ? (
        <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "8px 10px", borderRadius: 9, border: "1px solid var(--bd)", background: "var(--panel)", color: "var(--fg2)", fontSize: 12 }}>
          <span style={{ width: 6, height: 6, borderRadius: "50%", background: g.connected ? ACCENT : WARN, flexShrink: 0 }} />
          <span style={{ flex: 1, fontWeight: 600 }}>graphify · code graph</span>
          <span onClick={() => remove("lectern-graphify")} title="disconnect" style={{ cursor: "pointer", color: "var(--fg3)", fontSize: 15, lineHeight: 1 }}>×</span>
        </div>
      ) : (
        <button onClick={() => invoke<string | null>("pick_folder").then((p) => (p ? invoke("connect_graphify", { path: p }).then(() => { setMsg(null); onRefresh?.(); }) : null)).catch((e) => setMsg(String(e)))}
          style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 10px", borderRadius: 9, border: "1px solid var(--bd)", background: "var(--panel)", color: "var(--fg2)", cursor: "pointer", fontFamily: "inherit", textAlign: "left" }}>
          <span style={{ lineHeight: 1.35 }}><div style={{ fontWeight: 700, fontSize: 12.5, color: "var(--fg)" }}>Connect graphify</div><div style={{ fontSize: 10.5, color: "var(--fg3)" }}>Pick a repo — agents query its code graph via MCP</div></span>
        </button>
      ); })()}
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 6 }}>
        <div className="mono" style={{ fontSize: 11, fontWeight: 600, color: "var(--fg3)", letterSpacing: "0.06em", flexShrink: 0 }} title={detected.length ? `One click registers in: ${detected.join(", ")}` : undefined}>POPULAR SERVERS</div>
        <input value={q} onChange={(e) => { setQ(e.target.value); }} placeholder="Search servers…" spellCheck={false}
          style={{ flex: 1, height: 28, border: "1px solid var(--bd)", borderRadius: 8, background: "var(--bg)", color: "var(--fg)", fontSize: 12, padding: "0 10px", outline: "none", fontFamily: "inherit", minWidth: 0 }} />
      </div>
      {detected.length > 1 && (
        <div style={{ fontSize: 11.5, color: "var(--fg3)", marginTop: -4 }}>One click registers a server in every agent you have: {detected.join(" + ")}.</div>
      )}
      {(() => {
        const needle = q.trim().toLowerCase();
        const hits = MCP_CATALOG.filter((c) => !needle || c.name.toLowerCase().includes(needle) || c.desc.toLowerCase().includes(needle) || c.key.includes(needle));
        const visible = needle || showAll ? hits : hits.slice(0, 6);
        return (
          <>
            {visible.map((c) => (
              <CatalogRow key={c.key} entry={c} added={mcp.some((m) => m.name.toLowerCase() === c.key)} onDone={(note) => { setMsg(note || null); onRefresh?.(); }} onErr={setMsg} />
            ))}
            {needle && hits.length === 0 && <div style={{ fontSize: 12, color: "var(--fg3)" }}>Nothing matches "{q}" — add it as a custom server below.</div>}
            {!needle && hits.length > 6 && (
              <button onClick={() => (onBrowse ? onBrowse() : setShowAll((v) => !v))}
                style={{ border: "none", background: "transparent", color: "var(--fg2)", fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit", padding: "2px 0", alignSelf: "flex-start" }}>
                {onBrowse ? `Browse all ${hits.length} servers + channels →` : showAll ? "Show fewer" : `Show all ${hits.length} servers`}
              </button>
            )}
          </>
        );
      })()}
      {mcp.filter((m) => m.name !== "lectern-brain" && m.name !== "lectern-graphify").length === 0 && <div style={{ fontSize: 12, color: "var(--fg3)", lineHeight: 1.5 }}>No MCP servers yet. Add one to give the agent extra tools — files, APIs, or another service it can call.</div>}
      {mcp.filter((m) => m.name !== "lectern-brain" && m.name !== "lectern-graphify").map((m) => (
        <div key={m.name} style={{ border: "1px solid var(--bd)", borderRadius: 9, padding: "8px 10px", background: "var(--panel)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <span title={m.connected ? "connected" : "not connected"} style={{ width: 6, height: 6, borderRadius: "50%", flexShrink: 0, background: m.connected ? ACCENT : DANGER }} />
            <span style={{ flex: 1, fontSize: 12.5, fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{m.name}</span>
            <span onClick={() => remove(m.name)} title="remove" style={{ cursor: "pointer", color: "var(--fg3)", fontSize: 15, lineHeight: 1, padding: "0 2px" }}>×</span>
          </div>
          {m.detail && <div className="mono" style={{ fontSize: 10.5, color: "var(--fg3)", marginTop: 4, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={m.detail}>{m.detail}</div>}
        </div>
      ))}
      {adding ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="name (e.g. telegram)" spellCheck={false} className="mono" style={{ ...ctrl, width: "100%" }} />
          <input value={cmd} onChange={(e) => setCmd(e.target.value)} placeholder="command or https URL" spellCheck={false} className="mono" style={{ ...ctrl, width: "100%" }} />
          <div style={{ display: "flex", gap: 6 }}>
            <button onClick={add} style={{ flex: 1, height: 28, borderRadius: 7, border: "none", background: "var(--btn)", color: "var(--btnfg)", fontSize: 12, fontWeight: 700, cursor: "pointer", fontFamily: "inherit" }}>Add</button>
            <button onClick={() => { setAdding(false); setMsg(null); }} style={{ height: 28, padding: "0 10px", borderRadius: 7, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg2)", fontSize: 12, cursor: "pointer", fontFamily: "inherit" }}>Cancel</button>
          </div>
        </div>
      ) : (
        <button onClick={() => setAdding(true)} style={{ height: 30, borderRadius: 8, border: "1px dashed var(--bd)", background: "transparent", color: "var(--fg2)", fontSize: 12.5, cursor: "pointer", fontFamily: "inherit" }}>+ Add MCP server</button>
      )}
      {msg && <div style={{ fontSize: 11, color: DANGER, lineHeight: 1.4, wordBreak: "break-word" }}>{msg}</div>}
    </div>
  );
}


/* The user model (user.md): free-text preferences injected into every run
   alongside the machine profile — "[Lectern user] honor these preferences". */
function AboutLecternCard() {
  const [meta, setMeta] = useState<{ version: string; license: string; repo: string } | null>(null);
  useEffect(() => { invoke<{ version: string; license: string; repo: string }>("app_meta").then(setMeta).catch(() => {}); }, []);
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 14, padding: "14px 16px", border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)" }}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontWeight: 700, fontSize: 13.5 }}>
          Lectern {meta ? `v${meta.version}` : ""} <span style={{ fontWeight: 500, color: "var(--fg3)" }}>· open source · {meta?.license ?? "Apache-2.0"}</span>
        </div>
        <div style={{ fontSize: 12, color: "var(--fg2)", marginTop: 3, lineHeight: 1.5 }}>
          Built in public — the whole engine, app, and TUI live on GitHub. Stars, issues, and PRs all welcome.
        </div>
      </div>
      <button onClick={() => invoke("open_url", { url: "https://github.com/ShrimpScript/lectern" }).catch(() => {})}
        style={{ height: 30, padding: "0 13px", borderRadius: 8, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg)", fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit", display: "inline-flex", alignItems: "center", gap: 7, flexShrink: 0 }}>
        <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor" aria-hidden><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.42 7.42 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" /></svg>
        View on GitHub
      </button>
    </div>
  );
}

function AboutYouCard() {
  const [text, setText] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    invoke<string>("get_user_profile").then((t) => { setText(t); setLoaded(true); }).catch(() => setLoaded(true));
  }, []);
  const save = () => {
    invoke("set_user_profile", { content: text }).then(() => { setSaved(true); setTimeout(() => setSaved(false), 1600); }).catch(() => {});
  };
  return (
    <div style={{ border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", padding: "15px 16px", display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={{ fontSize: 13, color: "var(--fg2)", lineHeight: 1.55 }}>
        Tell your agents how you work — stack and tools you prefer, how strict reviews should be,
        how verbose answers should be, anything they should always honor. Injected into every run,
        stays on this machine. Tip: ask an agent to <i>“update my user profile from our recent sessions”</i> anytime.
      </div>
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={"e.g.\n- TypeScript + Rust; prefer dependency-free solutions\n- Terse answers; show the diff, skip the lecture\n- Never touch .env files"}
        spellCheck={false}
        disabled={!loaded}
        style={{ minHeight: 110, resize: "vertical", border: "1px solid var(--bd)", borderRadius: 9, background: "var(--bg)", color: "var(--fg)", fontSize: 13, lineHeight: 1.55, padding: "10px 12px", outline: "none", fontFamily: "inherit" }}
      />
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <button onClick={save} style={{ height: 30, padding: "0 14px", borderRadius: 8, border: "none", background: "var(--btn)", color: "var(--btnfg)", fontSize: 12.5, fontWeight: 700, cursor: "pointer", fontFamily: "inherit" }}>Save</button>
        {saved && <span className="lectern-fadein" style={{ fontSize: 12, color: "var(--fg3)" }}>saved — applies to the next run</span>}
      </div>
    </div>
  );
}


/* Mission B5 — the routing config made visible: which model handles what (the
   Conductor's planner + every routed task use these same rules), where the file
   lives, and a one-click way to open it. Edits apply live — the engine re-reads
   the file on every route. */
type RoutingSummary = { path: string; default_label: string; use_classifier: boolean; rules: { label: string; keywords: string[]; max_words: number | null; target: string }[] };
function RoutingConfigCard() {
  const [sum, setSum] = useState<RoutingSummary | null>(null);
  useEffect(() => { invoke<RoutingSummary>("routing_summary").then(setSum).catch(() => {}); }, []);
  if (!sum) return null;
  return (
    <div style={{ border: "1px solid var(--bd)", borderRadius: 12, padding: "15px 16px", background: "var(--panel)", marginTop: 10, display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 12 }}>
        <div>
          <div style={{ fontWeight: 600, fontSize: 15 }}>Routing rules</div>
          <div style={{ fontSize: 13, color: "var(--fg2)", marginTop: 3, lineHeight: 1.5 }}>
            How Lectern picks a model per task — the Conductor's planner and every <b style={{ color: "var(--fg)" }}>Auto</b> run follow these rules. Edit the file; changes apply on the very next run.
          </div>
        </div>
        <button onClick={() => invoke("open_config_file", { path: sum.path }).catch(() => {})}
          style={{ flexShrink: 0, height: 30, padding: "0 13px", borderRadius: 8, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg)", fontSize: 12.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>Open file</button>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
        {sum.rules.map((r, i) => (
          <div key={i} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12.5 }}>
            <span style={{ color: "var(--fg)", fontWeight: 600, minWidth: 110 }}>{r.label}</span>
            <span style={{ color: "var(--fg3)", flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
              {r.keywords.length ? r.keywords.slice(0, 4).join(", ") + (r.keywords.length > 4 ? "…" : "") : ""}
              {r.max_words ? `${r.keywords.length ? " · " : ""}≤${r.max_words} words` : ""}
            </span>
            <span className="mono" style={{ fontSize: 10.5, color: "var(--fg2)", flexShrink: 0 }}>{r.target}</span>
          </div>
        ))}
        <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12.5 }}>
          <span style={{ color: "var(--fg)", fontWeight: 600, minWidth: 110 }}>Everything else</span>
          <span style={{ color: "var(--fg3)", flex: 1 }}>{sum.use_classifier ? "fast classifier decides" : "falls back to"}</span>
          <span className="mono" style={{ fontSize: 10.5, color: "var(--fg2)", flexShrink: 0 }}>{sum.default_label}</span>
        </div>
      </div>
      <div className="mono" style={{ fontSize: 10.5, color: "var(--fg3)", borderTop: "1px solid var(--bd2)", paddingTop: 9, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{sum.path}</div>
    </div>
  );
}


/* Copy a server's registered command — the "how do I run this elsewhere" answer. */
function CopyCmd({ cmd }: { cmd: string }) {
  const [ok, setOk] = useState(false);
  if (!cmd?.trim()) return null;
  return (
    <button title={`Copy command: ${cmd}`} onClick={() => { navigator.clipboard?.writeText(cmd).then(() => { setOk(true); setTimeout(() => setOk(false), 1400); }).catch(() => {}); }}
      style={{ border: "none", background: "transparent", color: ok ? "var(--fg)" : "var(--fg3)", cursor: "pointer", padding: 2, display: "inline-flex", flexShrink: 0 }}>
      {ok ? (
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden><path d="m4.5 12.5 5 5 10-11" /></svg>
      ) : (
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15V5a2 2 0 0 1 2-2h10" /></svg>
      )}
    </button>
  );
}


/* Mission C5 — Channels are NOT MCP: messaging apps that can prompt/notify your
   Claude Code session remotely. Read-only status here; pairing + allowlists stay
   in the CLI on purpose (inbound remote messages are a prompt-injection surface,
   so Lectern never approves anyone from the UI). */
type ChannelStatus = { name: string; configured: boolean; allowed: number; pending: number; dm_policy: string };
function RemoteAccessCard({ onBrowse }: { onBrowse?: () => void }) {
  const [channels, setChannels] = useState<ChannelStatus[] | null>(null);
  useEffect(() => { invoke<ChannelStatus[]>("channels_status").then(setChannels).catch(() => setChannels([])); }, []);
  const row = (c: ChannelStatus) => (
    <div key={c.name} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 14px", borderTop: "1px solid var(--bd2)" }}>
      <span style={{ width: 30, height: 30, flexShrink: 0, border: "1px solid var(--bd2)", borderRadius: 8, display: "inline-flex", alignItems: "center", justifyContent: "center", color: "var(--fg)" }}>
        {mcpIconFor(c.name, 15) ?? <Icon name="agent" size={15} />}
      </span>
      <div style={{ flex: 1, minWidth: 0, lineHeight: 1.4 }}>
        <div style={{ fontWeight: 600, fontSize: 13.5, textTransform: "capitalize" }}>{c.name}</div>
        <div style={{ fontSize: 11.5, color: "var(--fg3)" }}>
          {c.configured
            ? `paired · ${c.allowed} allowed sender${c.allowed === 1 ? "" : "s"} · DM policy: ${c.dm_policy}${c.pending ? ` · ${c.pending} pending request${c.pending === 1 ? "" : "s"} (approve in the CLI)` : ""}`
            : "not set up"}
        </div>
      </div>
      <span style={{ display: "inline-flex", alignItems: "center", gap: 6, flexShrink: 0, fontSize: 11.5, fontWeight: 600, color: c.configured ? "var(--fg)" : "var(--fg3)", border: "1px solid var(--bd)", borderRadius: 999, padding: "4px 11px" }}>
        <span style={{ width: 6, height: 6, borderRadius: "50%", background: c.configured ? ACCENT : "var(--bd2)" }} />
        {c.configured ? "Active" : "Off"}
      </span>
    </div>
  );
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={{ fontSize: 13, color: "var(--fg2)", lineHeight: 1.6 }}>
        Different from MCP tools: <b style={{ color: "var(--fg)" }}>channels</b> let a messaging app reach your
        agent — send it tasks from your phone, get completion pings back. Powered by Claude Code's channel
        system; Telegram today, more as Claude adds them.
      </div>
      <div style={{ border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", overflow: "hidden" }}>
        {channels === null ? (
          <div style={{ padding: "12px 14px", fontSize: 12, color: "var(--fg3)" }}>Checking…</div>
        ) : channels.length === 0 ? (
          <div style={{ padding: "12px 14px", fontSize: 12.5, color: "var(--fg2)", lineHeight: 1.55 }}>
            No channels yet. In a terminal, run <span className="mono" style={{ fontSize: 11.5 }}>claude</span> and
            type <span className="mono" style={{ fontSize: 11.5 }}>/telegram:configure</span> — it walks you through
            creating a bot and pairing your phone.
          </div>
        ) : (
          <div style={{ marginTop: -1 }}>{channels.map(row)}</div>
        )}
      </div>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10 }}>
        <div style={{ fontSize: 11, color: "var(--fg3)", lineHeight: 1.5, flex: 1 }}>
          Security: incoming senders must be on the allowlist you approve in the CLI — Lectern never approves
          anyone from here, and pairing requests in a chat message are always ignored.
        </div>
        {onBrowse && (
          <button onClick={onBrowse} style={{ flexShrink: 0, border: "none", background: "transparent", color: "var(--fg2)", fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit", padding: 0 }}>Browse channels →</button>
        )}
      </div>
    </div>
  );
}


/* Mission D4 — theme manager: shareable JSON themes in ~/.lectern/themes.
   The built-in Light/Dark are immutable and always the fallback — a broken or
   deleted custom theme can never brick the UI. */
type ThemeRow = { file: string; name: string; base: string; valid: boolean };
function ThemeManagerCard({ prefs, onPrefs }: { prefs: Prefs; onPrefs: (p: Partial<Prefs>) => void }) {
  const [themes, setThemes] = useState<ThemeRow[]>([]);
  const [note, setNote] = useState<string | null>(null);
  const [naming, setNaming] = useState(false);
  const [newName, setNewName] = useState("");
  const fileRef = useRef<HTMLInputElement | null>(null);
  const load = () => invoke<ThemeRow[]>("list_themes").then(setThemes).catch(() => {});
  useEffect(() => { load(); }, []);
  const toast = (m: string) => { setNote(m); setTimeout(() => setNote(null), 3500); };
  const starter = (name: string) => JSON.stringify({
    lectern_theme: 1,
    name,
    base: prefs.theme,
    _note: `Override any of: ${THEME_VAR_WHITELIST.join(", ")}. Colors are CSS values. Save, then re-select the theme (or restart) to apply.`,
    vars: { "--accent": "#7fd4a0", "--btn": "#1f6feb", "--btnfg": "#ffffff" },
  }, null, 2);
  const create = () => {
    const nm = newName.trim(); if (!nm) return;
    const file = `${nm.toLowerCase().replace(/[^a-z0-9]+/g, "-")}.json`;
    invoke<string>("save_theme_file", { file, content: starter(nm), overwrite: false })
      .then((p2) => { setNaming(false); setNewName(""); load(); invoke("open_config_file", { path: p2 }).catch(() => {}); toast("Created — edit the file, then click Use."); })
      .catch((e) => toast(String(e)));
  };
  const importTheme = (f: File) => {
    f.text().then((t) => {
      try { const d = JSON.parse(t); if (d?.lectern_theme !== 1) throw new Error(); } catch { toast("Not a Lectern theme file."); return; }
      invoke<string>("save_theme_file", { file: f.name, content: t, overwrite: true }).then(() => { load(); toast(`Imported ${f.name}`); }).catch((e) => toast(String(e)));
    });
  };
  const exportTheme = (t: ThemeRow) => {
    invoke<string>("read_theme", { file: t.file })
      .then((content) => invoke<string>("save_chat_export", { filename: t.file, content }))
      .then((p2) => toast(`Exported to ${p2}`)).catch((e) => toast(String(e)));
  };
  const rowStyle: React.CSSProperties = { display: "flex", alignItems: "center", gap: 10, padding: "10px 12px", borderTop: "1px solid var(--bd2)", fontSize: 12.5 };
  const ghost: React.CSSProperties = { border: "none", background: "transparent", color: "var(--fg3)", fontSize: 11.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit", padding: 0 };
  return (
    <div style={{ border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", marginTop: 10, overflow: "hidden" }}>
      <div style={{ padding: "13px 14px 10px", display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 12 }}>
        <div>
          <div style={{ fontWeight: 600, fontSize: 15 }}>Custom themes</div>
          <div style={{ fontSize: 12.5, color: "var(--fg2)", marginTop: 2, lineHeight: 1.5 }}>Shareable JSON files in <span className="mono" style={{ fontSize: 11 }}>~/.lectern/themes</span>. The built-in Light/Dark can't be broken — anything invalid falls back to them.</div>
        </div>
        <div style={{ display: "flex", gap: 8, flexShrink: 0 }}>
          <button onClick={() => fileRef.current?.click()} style={{ height: 28, padding: "0 11px", borderRadius: 8, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg)", fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>Import…</button>
          <button onClick={() => setNaming((v) => !v)} style={{ height: 28, padding: "0 11px", borderRadius: 8, border: "none", background: "var(--btn)", color: "var(--btnfg)", fontSize: 12, fontWeight: 700, cursor: "pointer", fontFamily: "inherit" }}>New theme</button>
        </div>
      </div>
      {naming && (
        <div style={{ display: "flex", gap: 6, padding: "0 14px 12px" }}>
          <input autoFocus value={newName} onChange={(e) => setNewName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") create(); if (e.key === "Escape") setNaming(false); }}
            placeholder="Theme name" spellCheck={false} style={{ ...ctrl, flex: 1 }} />
          <button onClick={create} style={{ height: 30, padding: "0 12px", borderRadius: 8, border: "none", background: "var(--btn)", color: "var(--btnfg)", fontSize: 12, fontWeight: 700, cursor: "pointer", fontFamily: "inherit" }}>Create</button>
        </div>
      )}
      <div style={rowStyle}>
        <span style={{ flex: 1, fontWeight: 600 }}>Built-in default ({prefs.theme})</span>
        {prefs.custom_theme === null ? <span style={{ fontSize: 11, color: "var(--fg3)" }}>active ✓</span> : (
          <button style={ghost} onClick={() => onPrefs({ custom_theme: null })}>Use</button>
        )}
      </div>
      {themes.map((t) => (
        <div key={t.file} style={rowStyle}>
          <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontWeight: 600, color: t.valid ? "var(--fg)" : DANGER }}>{t.name}{t.valid ? "" : " (invalid)"}</span>
          <span className="mono" style={{ fontSize: 10, color: "var(--fg3)" }}>{t.base}</span>
          {prefs.custom_theme === t.file ? <span style={{ fontSize: 11, color: "var(--fg3)" }}>active ✓</span> : (
            t.valid && <button style={ghost} onClick={() => onPrefs({ custom_theme: t.file })}>Use</button>
          )}
          <button style={ghost} onClick={() => exportTheme(t)}>Export</button>
        </div>
      ))}
      {note && <div className="lectern-fadein" style={{ padding: "8px 14px", fontSize: 11, color: "var(--fg3)", borderTop: "1px solid var(--bd2)" }}>{note}</div>}
      <input ref={fileRef} type="file" accept=".json" style={{ display: "none" }} onChange={(e) => { const f = e.target.files?.[0]; if (f) importTheme(f); e.target.value = ""; }} />
    </div>
  );
}
