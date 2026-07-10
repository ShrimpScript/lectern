import { lazy, memo, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { invoke, Channel, convertFileSrc } from "@tauri-apps/api/core";
import { AntigravityIcon, ClaudeIcon, providerIcon } from "./BrandIcons";
import { Onboarding } from "./Onboarding";
import {
  Paperclip, Mic, ArrowUp, ChevronDown, Home, MessageSquare, Sparkles,
  LayoutGrid, Brain as BrainGlyph, Calendar, SlidersHorizontal, User,
  Search, Folder, PanelLeftClose, PanelLeft, PanelRight, PanelRightClose,
  SquarePen, GitBranch, BarChart3, MoreHorizontal, X, Globe, Code,
  ChevronLeft, ChevronRight, AlertTriangle, Copy, Check, RotateCcw,
  type LucideIcon,
} from "lucide-react";
// Lazy-loaded: CodeMirror + its language packs are heavy and only needed when a
// file is opened, so they're split into an on-demand chunk (smaller startup bundle).
const CodeEditor = lazy(() => import("./CodeEditor").then((m) => ({ default: m.CodeEditor })));
const Marketplace = lazy(() => import("./Marketplace").then((m) => ({ default: m.Marketplace })));
const Brain = lazy(() => import("./Brain").then((m) => ({ default: m.Brain })));
const Settings = lazy(() => import("./Settings").then((m) => ({ default: m.Settings })));
const Schedule = lazy(() => import("./Schedule").then((m) => ({ default: m.Schedule })));
const Usage = lazy(() => import("./Usage"));
const Connect = lazy(() => import("./Connect"));
const TerminalDrawer = lazy(() => import("./TerminalDrawer"));

/** Mirrors the engine's normalized AgentEvent (serde tag "type", snake_case). */
export type Ev =
  | { type: "user"; text: string; images?: string[]; oneShot?: boolean; skill?: string }
  | { type: "thinking" }
  | { type: "thought"; summary: string; recalls: string[] }
  | { type: "skill_applied"; name: string; why: string }
  | { type: "checkpoint"; id: string; label: string }
  | { type: "model_routed"; model: string; reason: string }
  | { type: "plan"; steps: { done: boolean; text: string }[] }
  | { type: "file_edit"; path: string; added: number; removed: number; preview: { kind: string; text: string }[] }
  | { type: "terminal"; command: string; output: string; exit_code: number }
  | { type: "message"; text: string; streaming?: boolean }
  | { type: "message_delta"; text: string }
  | { type: "usage"; input_tokens: number; output_tokens: number }
  | { type: "limit_hit"; reason: string }
  | { type: "error"; message: string }
  | { type: "done" }
  | { type: string; [k: string]: unknown };

// Append an event, merging streamed `message_delta` chunks into one growing message bubble
// so assistant text types out live instead of popping in all at once.
export function pushEv(events: Ev[], ev: Ev): Ev[] {
  if (ev.type === "message_delta") {
    const text = String((ev as { text?: string }).text ?? "");
    const last = events[events.length - 1];
    if (last && last.type === "message" && (last as { streaming?: boolean }).streaming) {
      return [...events.slice(0, -1), { ...last, text: ((last as { text: string }).text ?? "") + text }];
    }
    return [...events, { type: "message", text, streaming: true }];
  }
  return [...events, ev];
}

type RunSummary = { session_id: string; changes: number; applied: boolean; limit_hit: boolean; input_tokens: number; output_tokens: number };
export type BackendInfo = { id: string; label: string; available: boolean; detail: string };
type DoctorInfo = { claude_available: boolean; claude_version: string | null; default_dir: string };
type FileEntry = { name: string; dir: boolean };
export type SkillInfo = { name: string; description: string; triggers: string[]; uses: number; steps: string[]; rules: string[]; gui: boolean; ok: number; err: number; paused: boolean };
export type RegistryEntry = { id: string; name: string; description: string; triggers: string[]; author: string | null; version: number; kind: string; sha256?: string | null; official?: boolean; external?: boolean; publisher?: string | null; source_url?: string | null };
export type SkillBundle = { name: string; description: string; triggers: string[]; rules: string[]; steps: string[]; author: string | null; version: number; docs?: string | null };
type AccountInfo = { signed_in: boolean; base_url: string | null; plan: string | null };
type PastedImage = { path: string; url: string };

type Session = {
  id: string;
  title: string;
  path: string;
  backend: string;
  model: string;
  apply: boolean;
  yolo: boolean;
  draft: string;
  images?: PastedImage[];
  files?: string[];
  attachedSkill?: string;
  attachedSkillGui?: boolean;
  queued?: string; // a follow-up composed while a run was busy; promoted to the composer when it finishes
  mode?: "conduct" | "one-shot";
  view?: "clean" | "verbose"; // per-chat output view override (D1); unset = Settings default
  personalAgent?: boolean;
  events: Ev[];
  busy: boolean;
  summary?: RunSummary;
  pinned?: boolean;
  project?: string;
  updatedAt?: number;
};

type Screen = "chat" | "agent" | "marketplace" | "brain" | "usage" | "schedule" | "settings" | "profile" | "connect";
export type ThemeName = "dark" | "light";
export const THEME_VAR_WHITELIST = ["--bg", "--panel", "--panel2", "--elev", "--bd", "--bd2", "--fg", "--fg2", "--fg3", "--hov", "--btn", "--btnfg", "--backdrop", "--accent", "--chrome", "--tree"];
export type Prefs = { theme: ThemeName; default_backend: string; default_model: string; default_apply: boolean; onboarded: boolean; clean_output: boolean; custom_theme: string | null; whats_new_seen?: string | null };
export type ScheduleInfo = { id: string; prompt: string; backend: string; apply: boolean; run_at: number; reason: string; status: string };
type AgentSkill = { name: string; description: string };
export type McpServer = { name: string; detail: string; connected: boolean; oc: boolean; agy: boolean };
type ModelInfo = { id: string; label: string };
export type BrainNodeT = { id: string; label: string; kind: string; weight: number };
export type BrainGraphT = { nodes: BrainNodeT[]; edges: { from: string; to: string }[]; skills: number; memory: number; sessions: number };

// ── design tokens (from Lectern-Brain/99-Design-Source) ──────────────────────
const THEMES: Record<ThemeName, Record<string, string>> = {
  dark: { backdrop: "#050506", bg: "#0a0a0b", panel: "#141416", panel2: "#1d1d20", elev: "#0e0e10", chrome: "#161618", bd: "rgba(255,255,255,.10)", bd2: "rgba(255,255,255,.06)", fg: "#f4f4f2", fg2: "#9a9a96", fg3: "#66665f", btn: "#f4f4f2", btnfg: "#0a0a0a", hov: "rgba(255,255,255,.06)", tree: "#0c0c0e", diffAddBg: "#15241a", diffAddFg: "#9fe0ad", diffRmBg: "#26171a", diffRmFg: "#e58a97" },
  light: { backdrop: "#d6d6d3", bg: "#f6f6f5", panel: "#ffffff", panel2: "#eeeeec", elev: "#fbfbfa", chrome: "#eaeae7", bd: "rgba(0,0,0,.13)", bd2: "rgba(0,0,0,.06)", fg: "#18181b", fg2: "#5d5d58", fg3: "#8c8c86", btn: "#18181b", btnfg: "#fafafa", hov: "rgba(0,0,0,.05)", tree: "#f0f0ed", diffAddBg: "#e6f4ea", diffAddFg: "#1a7f37", diffRmBg: "#fce9ec", diffRmFg: "#b32639" },
};
const themeStyle = (t: ThemeName) => Object.fromEntries(Object.entries(THEMES[t]).map(([k, v]) => [`--${k}`, v])) as React.CSSProperties;

// Monochrome: "accent" = the theme foreground (black on light, near-white on dark). No green.
export const ACCENT = "var(--fg)";
export const DANGER = "#e5687a";
export const WARN = "#e5a05f";
const COL = 800;
// Primary nav stays tight so newcomers aren't overwhelmed; power features live under
// a collapsed "Advanced" group.
const NAV_PRIMARY: { id: Screen; label: string; icon: string }[] = [
  { id: "chat", label: "Chat", icon: "chat" },
  { id: "agent", label: "Personal Agent", icon: "agent" },
  { id: "brain", label: "Brain", icon: "brain" },
  { id: "usage", label: "Usage", icon: "usage" },
  { id: "settings", label: "Settings", icon: "settings" },
];
const NAV_ADVANCED: { id: Screen; label: string; icon: string }[] = [
  { id: "marketplace", label: "Hub", icon: "market" },
  { id: "schedule", label: "Schedule", icon: "schedule" },
  { id: "profile", label: "Profile", icon: "profile" },
];

export const ctrl: React.CSSProperties = { height: 30, boxSizing: "border-box", fontSize: 12, lineHeight: "28px", color: "var(--fg)", background: "var(--bg)", border: "1px solid var(--bd)", borderRadius: 8, padding: "0 10px", outline: "none" };

type SlashCmd = { cmd: string; id: string; desc: string; ready: boolean };
const SLASH: SlashCmd[] = [
  { cmd: "/clear", id: "clear", desc: "Clear this conversation", ready: true },
  { cmd: "/plan", id: "plan", desc: "Switch to plan mode — review before edits", ready: true },
  { cmd: "/apply", id: "apply", desc: "Switch to apply mode — edits land in your repo", ready: true },
  { cmd: "/one-shot", id: "one-shot", desc: "Autonomous build — toggle the mode (or /one-shot <brief> for one run)", ready: true },
  { cmd: "/conduct", id: "conduct", desc: "Conductor — toggle orchestration mode (or /conduct <task> for one run)", ready: true },
  { cmd: "/record", id: "record", desc: "Record a demonstration → reusable skill (toggle)", ready: true },
  { cmd: "/brief", id: "brief", desc: "Draft a structured task brief (goal · acceptance · constraints · test)", ready: true },
  { cmd: "/help", id: "help", desc: "List available commands", ready: true },
  { cmd: "/skill", id: "skill", desc: "Attach a learned skill: /skill [name]", ready: true },
  { cmd: "/mcp", id: "mcp", desc: "Target an MCP server: /mcp [server]", ready: true },
];
const SLASH_HELP = [
  "**Commands**",
  "- `/one-shot <brief>` — autonomous build: a short brief, Claude plans the full scope and builds a complete product (auto-applies, runs a while)",
  "- `/conduct <task>` — Conductor: plans the task, then hands each sub-task to the model that excels at it (Gemini Flash for fast work, Opus for reasoning, …); auto-applies",
  "- `/record` — capture your clicks/typing across the screen, then save it as a reusable skill (run `/record` again to stop)",
  "- `/brief` — draft a structured task brief (goal · acceptance · constraints · test) to fill in and send",
  "- `/skill [name]` — attach a learned skill (recorded ones replay; procedural ones guide)",
  "- `/mcp [server]` — target a connected MCP server for the next message",
  "- `/clear` — clear this conversation",
  "- `/plan` — review changes before they're written",
  "- `/apply` — let Claude Code apply edits",
  "- `/help` — this list",
].join("\n");

// Expansive autonomous-build preamble for /one-shot (short brief → complete product).
const ONE_SHOT_PREAMBLE =
  "You are in ONE-SHOT AUTONOMOUS BUILD mode. Treat the brief below as the seed for a complete, " +
  "production-quality product — not a minimal answer. First think through and plan the full scope " +
  "(architecture, files, data, edge cases, error states, and polish). Then implement it end to end: " +
  "create every file, wire it together, handle edge cases, add reasonable tests, and refine until it " +
  "genuinely works and feels finished. Keep going autonomously through the whole plan — do not stop at " +
  "a skeleton or ask for confirmation between steps. When done, summarize what you built and how to run it.";

// Equips the Personal Agent to drive the whole machine (computer-use on X11).
const PERSONAL_AGENT_PREAMBLE =
  "You are the user's PERSONAL DESKTOP AGENT on a Linux X11 machine (DISPLAY=:0). You can fully control the " +
  "computer's GUI, not just edit code: take screenshots (scrot / `import -window <id>`), move/click/type (xdotool), " +
  "manage windows (wmctrl), and open apps. Use your `remote-desktop` skill for fast, precise control, and apply any " +
  "relevant Lectern memory/skills shown above. Work autonomously — actually perform the task on the live desktop and " +
  "report what you did with a verifying screenshot. If a connected MCP tool (telegram, etc.) fits the task, use it.";

// Cache the file tree per path so switching tabs/screens shows it instantly
// (then a background refresh keeps it current).
const treeCache = new Map<string, FileEntry[]>();

// The agent's own native skills (Claude Code), fetched once for the slash menu.
const agentSkillsCache: { v: AgentSkill[] | null } = { v: null };

let counter = 1;
const uid = () => `s${counter++}`;
const truncate = (s: string, n = 28) => (s.length > n ? s.slice(0, n - 1) + "…" : s);
const baseName = (p: string) => p.replace(/\/+$/, "").split("/").pop() || p || "—";

/* tmux-style session tiles. A binary split tree —
   every leaf hosts a full independent Chat; splits are 50/50 (resize later).
   Closing panes collapses the tree; one pane left = back to classic view. */
export type TileNode =
  | { kind: "leaf"; id: string; sessionId: string }
  | { kind: "split"; id: string; dir: "row" | "col"; a: TileNode; b: TileNode; ratio?: number };

function tileSetRatio(node: TileNode, splitId: string, ratio: number): TileNode {
  if (node.kind === "leaf") return node;
  if (node.id === splitId) return { ...node, ratio: Math.min(0.85, Math.max(0.15, ratio)) };
  return { ...node, a: tileSetRatio(node.a, splitId, ratio), b: tileSetRatio(node.b, splitId, ratio) };
}

function tileSplit(node: TileNode, leafId: string, dir: "row" | "col", newLeaf: TileNode): TileNode {
  if (node.kind === "leaf") {
    return node.id === leafId ? { kind: "split", id: uid(), dir, a: node, b: newLeaf } : node;
  }
  return { ...node, a: tileSplit(node.a, leafId, dir, newLeaf), b: tileSplit(node.b, leafId, dir, newLeaf) };
}
function tileClose(node: TileNode, leafId: string): TileNode | null {
  if (node.kind === "leaf") return node.id === leafId ? null : node;
  const a = tileClose(node.a, leafId);
  const b = tileClose(node.b, leafId);
  if (a && b) return { ...node, a, b };
  return a ?? b;
}
function tileSetSession(node: TileNode, leafId: string, sessionId: string): TileNode {
  if (node.kind === "leaf") return node.id === leafId ? { ...node, sessionId } : node;
  return { ...node, a: tileSetSession(node.a, leafId, sessionId), b: tileSetSession(node.b, leafId, sessionId) };
}

/* Fade out the index.html boot splash (it painted before any JS ran). A short
   floor keeps the logo-draw from strobing on very fast boots. */
function dismissSplash() {
  const el = document.getElementById("boot-splash");
  if (!el) return;
  const shownFor = performance.now();
  const wait = Math.max(0, 650 - shownFor);
  setTimeout(() => {
    el.classList.add("gone");
    setTimeout(() => el.remove(), 400);
  }, wait);
}

function newSession(path: string): Session {
  return { id: uid(), title: "New chat", path, backend: "auto", model: "", apply: false, yolo: false, draft: "", events: [], busy: false, updatedAt: Date.now() };
}

// The Personal Agent: one persistent, autonomous, computer-use session (shares the global brain).
// The agent defaults to Gemini Flash (Antigravity) — fast, snappy command/desktop work
// that doesn't over-think. Migrated to "auto" if Antigravity isn't installed (see effect).
const AGENT_FLASH_MODEL = "Gemini 3.5 Flash (High)";
function newAgentSession(path: string): Session {
  return { id: "agent", title: "Personal Agent", path, backend: "antigravity", model: AGENT_FLASH_MODEL, apply: true, yolo: true, draft: "", personalAgent: true, events: [], busy: false };
}

export function App() {
  const [prefs, setPrefsState] = useState<Prefs>({ theme: "light", default_backend: "auto", default_model: "", default_apply: false, onboarded: true, clean_output: false, custom_theme: null });
  const theme = prefs.theme;
  const savePrefs = (patch: Partial<Prefs>) => setPrefsState((p) => { const n = { ...p, ...patch }; invoke("set_prefs", { prefs: n }).catch(() => {}); return n; });
  const setTheme = (t: ThemeName) => savePrefs({ theme: t });
  /* Mission D4 — custom theme overlay: whitelisted vars from ~/.lectern/themes/<file>
     applied over the built-in base; ANY failure clears back to the immutable default. */
  const [customVars, setCustomVars] = useState<Record<string, string>>({});
  const [customBase, setCustomBase] = useState<ThemeName | null>(null);
  useEffect(() => {
    if (!prefs.custom_theme) { setCustomVars({}); setCustomBase(null); return; }
    invoke<string>("read_theme", { file: prefs.custom_theme })
      .then((text) => {
        const t = JSON.parse(text);
        if (t?.lectern_theme !== 1 || typeof t.vars !== "object") throw new Error("bad theme");
        const out: Record<string, string> = {};
        for (const [k, v] of Object.entries(t.vars as Record<string, string>)) {
          if (THEME_VAR_WHITELIST.includes(k) && typeof v === "string" && v.length < 64) out[k] = v;
        }
        setCustomVars(out);
        setCustomBase(t.base === "dark" ? "dark" : t.base === "light" ? "light" : null);
      })
      .catch(() => { setCustomVars({}); setCustomBase(null); });
  }, [prefs.custom_theme]);
  const effTheme = customBase ?? prefs.theme;
  // In-app updater — on launch, ask the signed release manifest whether a newer version
  // exists. In dev / offline / with no release yet this throws; ignore it so the banner
  // only ever appears on a real update.
  const updateRef = useRef<import("@tauri-apps/plugin-updater").Update | null>(null);
  const [updateInfo, setUpdateInfo] = useState<{ version: string; notes: string } | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  // "What's new" — after the user updates to a newer version, show that version's changelog
  // once. Distinct from the update banner above (which offers an install of a version you
  // don't have yet); this celebrates the version you're now running.
  const [whatsNew, setWhatsNew] = useState<{ version: string; sections: ChangeSection[] } | null>(null);
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const { check } = await import("@tauri-apps/plugin-updater");
        const up = await check();
        if (up && !cancelled) {
          updateRef.current = up;
          setUpdateInfo({ version: up.version, notes: up.body ?? "" });
        }
      } catch { /* no endpoint / offline / dev — nothing to surface */ }
    })();
    return () => { cancelled = true; };
  }, []);
  const runUpdate = async () => {
    const up = updateRef.current;
    if (!up || updateBusy) return;
    setUpdateBusy(true);
    try {
      await up.downloadAndInstall();
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch {
      setUpdateBusy(false);
    }
  };
  const [screen, setScreen] = useState<Screen>("chat");
  const [navOpen, setNavOpen] = useState(true);
  const [tiles, setTiles] = useState<TileNode | null>(null);
  const splitPane = (leafId: string | null, dir: "row" | "col") => {
    const fresh = newSession(homeRef.current);
    setSessions((prev) => [...prev, fresh]);
    const newLeaf: TileNode = { kind: "leaf", id: uid(), sessionId: fresh.id };
    setTiles((t) => {
      if (!t || leafId === null) {
        const current: TileNode = { kind: "leaf", id: uid(), sessionId: activeId };
        return { kind: "split", id: uid(), dir, a: t ?? current, b: newLeaf };
      }
      return tileSplit(t, leafId, dir, newLeaf);
    });
  };
  const closePane = (leafId: string) => setTiles((t) => (t ? tileClose(t, leafId) : null));
  useEffect(() => {
    // one pane left → classic view of that pane's session
    if (tiles && tiles.kind === "leaf") {
      setActiveId(tiles.sessionId);
      setTiles(null);
    }
  }, [tiles]);
  const [recording, setRecording] = useState(false);
  const [recordSteps, setRecordSteps] = useState<string[] | null>(null);
  const [skillsVersion, setSkillsVersion] = useState(0); // bump to refresh skill lists
  const [doctor, setDoctor] = useState<DoctorInfo | null>(null);
  const [backends, setBackends] = useState<BackendInfo[]>([]);
  const [claudeModels, setClaudeModels] = useState<ModelInfo[]>([]);
  const [ocModels, setOcModels] = useState<ModelInfo[]>([]);
  const [orModels, setOrModels] = useState<ModelInfo[]>([]);
  const [olModels, setOlModels] = useState<ModelInfo[]>([]);
  const [mcp, setMcp] = useState<McpServer[]>([]);
  const loadMcp = () => invoke<McpServer[]>("list_mcp").then(setMcp).catch(() => {});
  const [sessions, setSessions] = useState<Session[]>([newSession("")]);
  const [activeId, setActiveId] = useState<string>(sessions[0].id);
  const active = sessions.find((s) => s.id === activeId) ?? sessions[0];
  const claudeAvailable = doctor?.claude_available ?? false;
  // Any real provider (not the mock) — the setup nudge should only show when the
  // user genuinely has nothing connected, not merely when Claude Code is absent.
  const anyBackend = backends.some((b) => b.available && b.id !== "mock");
  const models = useMemo(() => modelOptions(claudeModels, ocModels, orModels, olModels), [claudeModels, ocModels, orModels, olModels]);
  const loaded = useRef(false);
  const homeRef = useRef(""); // user's home dir (for the agent's default workspace)

  useEffect(() => {
    invoke<DoctorInfo>("doctor").then((d) => {
      homeRef.current = d.default_dir ?? "";
      setDoctor(d);
      if (d.default_dir) setSessions((prev) => prev.map((s) => ((s.events.length === 0 && !s.path) || (s.personalAgent && !s.path) ? { ...s, path: d.default_dir } : s)));
    }).catch(() => {});
    // Boot perf: none of these gate first paint, and list_mcp alone can take
    // seconds (it pings every registered server). Let the UI land first.
    const deferred = setTimeout(() => {
      invoke<BackendInfo[]>("engine_backends").then(setBackends).catch(() => {});
      invoke<ModelInfo[]>("claude_models").then(setClaudeModels).catch(() => {});
      invoke<ModelInfo[]>("opencode_models").then(setOcModels).catch(() => {});
      invoke<ModelInfo[]>("models", { backend: "openrouter" }).then(setOrModels).catch(() => {});
      invoke<ModelInfo[]>("models", { backend: "ollama" }).then(setOlModels).catch(() => {});
      loadMcp();
    }, 700);
    invoke<Prefs>("get_prefs").then(async (p) => {
      setPrefsState(p);
      setSessions((prev) => prev.map((s) => (s.events.length === 0 ? { ...s, backend: p.default_backend, model: p.default_model, apply: p.default_apply } : s)));
      // Show the "what's new" once after an update to a newer version.
      try {
        const version = await invoke<string>("app_version");
        const seen = p.whats_new_seen ?? null;
        if (seen === null) {
          savePrefs({ whats_new_seen: version }); // fresh install — set the baseline, don't interrupt
        } else if (seen !== version) {
          const sections = await fetchWhatsNew(version);
          if (sections && sections.length) setWhatsNew({ version, sections });
          else savePrefs({ whats_new_seen: version }); // no notes to show — just advance
        }
      } catch { /* offline or no version — skip */ }
    }).catch(() => {});
    // Restore the saved session list (chats persist across restarts) + ensure the
    // persistent Personal Agent session exists.
    invoke<string>("get_sessions").then((raw) => {
      let restored: Session[] = [];
      try { restored = raw ? (JSON.parse(raw) as Session[]).map((s) => ({ ...s, busy: false })) : []; } catch { /* corrupt — fresh */ }
      if (!restored.some((s) => s.personalAgent)) restored.unshift(newAgentSession(homeRef.current));
      counter = Math.max(1, ...restored.map((s) => (parseInt(String(s.id).replace(/\D/g, ""), 10) || 0))) + 1;
      setSessions(restored);
      setActiveId(restored.find((s) => !s.personalAgent)?.id ?? restored[0].id);
      loaded.current = true;
      dismissSplash();
      reconcileWithStore(restored);
    }).catch(() => { loaded.current = true; dismissSplash(); });
      return () => clearTimeout(deferred);
  }, []);

  // Persist the session list (debounced) once the initial restore is done.
  // Perf: 1.5s debounce (was 0.5 — every keystroke scheduled a 20-session
  // When a run finishes, promote any follow-up queued during it into the composer so it's
  // right there ready to send (deterministic — it never auto-sends on its own).
  useEffect(() => {
    for (const s of sessions) {
      if (!s.busy && s.queued) {
        update(s.id, (x) => ({ ...x, draft: x.draft.trim() ? `${x.queued}\n${x.draft}` : x.queued!, queued: undefined }));
      }
    }
  }, [sessions]);

  // serialize) and never while a stream is running; the post-run state change
  // triggers the flush naturally.
  useEffect(() => {
    if (!loaded.current) return;
    if (sessions.some((s) => s.busy)) return;
    const t = setTimeout(() => {
      // Strip base64 image data (pending + in user events) so the file stays small.
      const slim = sessions.slice(-20).map((s) => ({
        ...s, busy: false, images: [],
        events: s.events.slice(-150).map((e) => (e.type === "user" && (e as any).images ? { ...e, images: undefined } : e)),
      }));
      invoke("save_sessions", { data: JSON.stringify(slim) }).catch(() => {});
      // Unification phase 2a: dual-write chat metadata into the ENGINE STORE.
      // A chat maps to its latest engine run (summary.session_id); meta carries
      // chat_id so phase 2b can group runs back into chats. JSON stays the
      // source of truth this phase; writes are diffed to avoid churn.
      for (const sess of sessions) {
        const storeId = sess.summary?.session_id;
        if (!storeId) continue;
        const meta = JSON.stringify({
          chat_id: sess.id, title: sess.title, model: sess.model, mode: sess.mode ?? null,
          view: sess.view ?? null, project: sess.project ?? null,
          personalAgent: !!sess.personalAgent, pinned: !!sess.pinned,
        });
        if (storeMetaCache.current.get(storeId) === meta) continue;
        storeMetaCache.current.set(storeId, meta);
        invoke("store_set_session_meta", { sessionId: storeId, metaJson: meta }).catch(() => {});
        invoke("store_rename_session", { sessionId: storeId, title: sess.title }).catch(() => {});
        invoke("store_pin_session", { sessionId: storeId, pinned: !!sess.pinned }).catch(() => {});
      }
    }, 1500);
    return () => clearTimeout(t);
  }, [sessions]);

  const storeMetaCache = useRef(new Map<string, string>()); // last dual-written meta per store id

  /* Unification phase 2b: after the JSON list loads, read the ENGINE STORE and
     reconcile — the store is authoritative for cross-surface changes.
     · matching chat (meta.chat_id) + store newer → adopt title/pinned
     · unknown store sessions created since the last reconcile (TUI/CLI work)
       → materialize as chats (events lazy-load on first open)
     The first run only plants the watermark so history doesn't flood in. */
  const reconcileWithStore = async (restored: Session[]) => {
    const watermarkKey = "lectern-store-watermark";
    const watermark = Number(localStorage.getItem(watermarkKey) ?? 0);
    const nowSec = Math.floor(Date.now() / 1000);
    const paths = [...new Set(restored.filter((s) => !s.personalAgent && s.path).map((s) => s.path))].slice(0, 8);
    type StoreRow = { id: string; title: string; backend: string; created_at: number; status: string; pinned: boolean; updated_at: number; meta: null | { chat_id?: string; model?: string; view?: string } };
    const adopt = new Map<string, { title: string; pinned: boolean; updated_at: number }>();
    const born: { row: StoreRow; path: string }[] = [];
    for (const path of paths) {
      const rows = await invoke<StoreRow[]>("store_sessions", { path, limit: 30 }).catch(() => [] as StoreRow[]);
      for (const row of rows) {
        const chatId = row.meta?.chat_id;
        if (chatId) adopt.set(chatId, { title: row.title, pinned: row.pinned, updated_at: row.updated_at });
        else if (watermark > 0 && row.created_at > watermark) born.push({ row, path });
      }
    }
    // one state pass → one re-render, regardless of how many surfaces changed things
    setSessions((prev) => {
      let out = prev.map((s) => {
        const a = adopt.get(s.id);
        if (!a || s.busy) return s;
        if (a.updated_at * 1000 <= (s.updatedAt ?? 0) || (s.title === a.title && !!s.pinned === a.pinned)) return s;
        console.info(`[unify] store wins for ${s.id}: title/pin from another surface`);
        return { ...s, title: a.title, pinned: a.pinned };
      });
      for (const { row, path } of born) {
        if (out.some((s) => s.summary?.session_id === row.id || s.id === `store-${row.id}`)) continue;
        console.info(`[unify] adopting ${row.backend} session from another surface: ${row.title}`);
        out = [...out, {
          ...newSession(path), id: `store-${row.id}`, title: row.title, backend: row.backend,
          pinned: row.pinned, updatedAt: row.updated_at * 1000,
          summary: { session_id: row.id, changes: 0, applied: false, limit_hit: false, input_tokens: 0, output_tokens: 0 },
        }];
      }
      return out;
    });
    for (const { row } of born) {
      invoke<Ev[]>("store_session_events", { sessionId: row.id })
        .then((evs) => setSessions((p2) => p2.map((s) => (s.id === `store-${row.id}` ? { ...s, events: evs } : s))))
        .catch(() => {});
    }
    localStorage.setItem(watermarkKey, String(nowSec));
  };

  const update = (id: string, fn: (s: Session) => Session) => setSessions((prev) => prev.map((s) => (s.id === id ? fn(s) : s)));

  function addSession() {
    const s = { ...newSession(active?.path ?? doctor?.default_dir ?? ""), backend: prefs.default_backend, model: prefs.default_model, apply: prefs.default_apply };
    setSessions((prev) => [...prev, s]);
    setActiveId(s.id);
    setScreen("chat");
  }
  function recheck() {
    invoke<DoctorInfo>("doctor").then(setDoctor).catch(() => {});
    invoke<BackendInfo[]>("engine_backends").then(setBackends).catch(() => {});
  }
  function openSession(id: string) { setActiveId(id); setScreen("chat"); }
  function navTo(sc: Screen) {
    if (sc === "agent") setActiveId("agent");
    else if (sc === "chat" && active.personalAgent) setActiveId(sessions.find((x) => !x.personalAgent)?.id ?? active.id);
    setScreen(sc);
  }
  function closeSession(id: string) {
    if (id === "agent") return; // the Personal Agent is permanent
    setSessions((prev) => {
      const next = prev.filter((s) => s.id !== id);
      const final = next.some((s) => !s.personalAgent) ? next : [...next, newSession(homeRef.current || (doctor?.default_dir ?? ""))];
      if (id === activeId) setActiveId((final.find((s) => !s.personalAgent) ?? final[0]).id);
      return final;
    });
  }
  // Reconcile the Personal Agent's backend with Antigravity availability: migrate the
  // legacy "auto" default → Gemini Flash when available; downgrade to auto if it's gone.
  useEffect(() => {
    if (!backends.length) return;
    const agyOk = backends.find((b) => b.id === "antigravity")?.available ?? false;
    setSessions((prev) => prev.map((s) => {
      if (!s.personalAgent) return s;
      if (agyOk && s.backend === "auto") return { ...s, backend: "antigravity", model: AGENT_FLASH_MODEL };
      if (!agyOk && s.backend === "antigravity") return { ...s, backend: "auto", model: "" };
      return s;
    }));
  }, [backends]);

  async function send(s: Session) {
    let prompt = s.draft.trim();
    // Steer while it works: if a run is active, don't drop the message — queue the
    // follow-up (concatenating multiple) and promote it to the composer when the run
    // finishes. Slash commands still fall through to their handlers.
    if (s.busy && prompt && !prompt.startsWith("/")) {
      update(s.id, (x) => ({ ...x, queued: (x.queued ? x.queued + "\n" : "") + prompt, draft: "" }));
      return;
    }
    const imgs = s.images ?? [];
    const files = s.files ?? [];
    const skill = s.attachedSkill;
    const agent = !!s.personalAgent;
    // The Personal Agent works on the whole machine — default its workspace to home.
    const path = s.path.trim() || (agent ? (doctor?.default_dir ?? "") : "");
    // Route slash commands to their action so they work however they're submitted (Send
    // button, Enter, or a trailing space that closed the menu) — not just via the menu.
    // /conduct + /one-shot take args and are handled below.
    const cmdMatch = prompt.match(/^\/([\w-]+)\b/);
    if (cmdMatch) {
      const c = SLASH.find((x) => x.cmd === `/${cmdMatch[1].toLowerCase()}`);
      // /skill and /mcp take an optional argument — route it through command()
      if (c && (c.id === "skill" || c.id === "mcp")) {
        const arg = prompt.slice(cmdMatch[0].length).trim();
        update(s.id, (x) => ({ ...x, draft: "" }));
        command(arg ? `${c.id}:${arg}` : c.id);
        return;
      }
      if (c && c.id !== "conduct" && c.id !== "one-shot") {
        update(s.id, (x) => ({ ...x, draft: "" }));
        command(c.ready ? c.id : `soon:${c.cmd}`);
        return;
      }
    }
    // Recorded GUI skill → replay deterministically (no agent), so it runs immediately
    // instead of a thinking model re-reasoning + second-guessing the macro.
    if (skill && s.attachedSkillGui) {
      if (s.busy) return;
      const skillPath = path || (doctor?.default_dir ?? "");
      const ch = new Channel<Ev>();
      ch.onmessage = (ev) => update(s.id, (x) => ({ ...x, events: pushEv(x.events, ev) }));
      update(s.id, (x) => ({ ...x, busy: true, summary: undefined, draft: "", attachedSkill: undefined, attachedSkillGui: undefined, updatedAt: Date.now(), title: x.title === "New chat" ? truncate(skill) : x.title, events: [...x.events, { type: "user", text: prompt, skill }] }));
      try {
        await invoke("replay_skill_session", { name: skill, path: skillPath, onEvent: ch });
        update(s.id, (x) => ({ ...x, busy: false }));
      } catch (e) {
        update(s.id, (x) => ({ ...x, busy: false, events: [...x.events, { type: "error", message: String(e) }] }));
      }
      return;
    }
    // /conduct + /one-shot — bare command TOGGLES the sticky mode (a composer
    // pill shows it; every send routes through it until toggled off); with args
    // it runs once. An explicit /one-shot beats an active conduct mode.
    const conductCmd = prompt.match(/^\/conduct\b\s*([\s\S]*)$/i);
    if (conductCmd && !conductCmd[1].trim()) {
      update(s.id, (x) => ({ ...x, mode: x.mode === "conduct" ? undefined : "conduct", draft: "" }));
      return;
    }
    const oneShotCmd = prompt.match(/^\/one-shot\b\s*([\s\S]*)$/i);
    if (oneShotCmd && !oneShotCmd[1].trim()) {
      update(s.id, (x) => ({ ...x, mode: x.mode === "one-shot" ? undefined : "one-shot", draft: "" }));
      return;
    }
    const conductTask = conductCmd ? conductCmd[1].trim() : s.mode === "conduct" && !oneShotCmd ? prompt : "";
    if (conductTask) {
      if (s.busy || !path) return;
      const ch = new Channel<Ev>();
      ch.onmessage = (ev) => update(s.id, (x) => ({ ...x, events: pushEv(x.events, ev) }));
      update(s.id, (x) => ({ ...x, busy: true, summary: undefined, draft: "", updatedAt: Date.now(), title: x.title === "New chat" ? truncate(conductTask) : x.title, events: [...x.events, { type: "user", text: conductTask, skill: "conduct" }] }));
      try {
        const summary = await invoke<RunSummary>("run_conductor_session", { prompt: conductTask, path, sessionId: s.id, onEvent: ch });
        update(s.id, (x) => ({ ...x, busy: false, summary }));
      } catch (e) {
        update(s.id, (x) => ({ ...x, busy: false, events: [...x.events, { type: "error", message: String(e) }] }));
      }
      return;
    }
    // one-shot: autonomous expansive build — auto-apply + skip permissions.
    // Explicit "/one-shot <brief>" or any send while one-shot mode is on.
    let apply = s.apply, yolo = s.yolo, oneShot = false;
    if (oneShotCmd) {
      oneShot = true; apply = true; yolo = true; prompt = oneShotCmd[1].trim();
    } else if (s.mode === "one-shot" && prompt) {
      oneShot = true; apply = true; yolo = true;
    }
    if ((!prompt && imgs.length === 0 && files.length === 0 && !skill) || s.busy || !path) return;
    if (skill || agent) { apply = true; yolo = true; } // skills + the agent run autonomously
    let basePrompt = oneShot ? `${ONE_SHOT_PREAMBLE}\n\nBrief: ${prompt}` : prompt;
    if (skill) {
      basePrompt = `Use your "${skill}" skill now: load it and execute it directly — everything you need is in the skill, so don't load other skills, take screenshots, or ask questions; just run it and report the result.${prompt ? ` Context: ${prompt}` : ""}`;
    }
    if (agent) {
      basePrompt = `${PERSONAL_AGENT_PREAMBLE}\n\nTask: ${prompt || "(see attached)"}`;
    }
    // Claude Code reads attached files by path via its Read tool, so reference the paths.
    const attached = [...imgs.map((i) => i.path), ...files];
    const fullPrompt = attached.length
      ? `${basePrompt}\n\nAttached file${attached.length > 1 ? "s" : ""} (read ${attached.length > 1 ? "them" : "it"} with your Read tool):\n${attached.join("\n")}`
      : basePrompt;
    const ch = new Channel<Ev>();
    ch.onmessage = (ev) => update(s.id, (x) => ({ ...x, events: pushEv(x.events, ev) }));
    update(s.id, (x) => ({ ...x, busy: true, summary: undefined, draft: "", images: [], files: [], attachedSkill: undefined, updatedAt: Date.now(), title: x.title === "New chat" ? truncate(prompt || skill || "attachment") : x.title, events: [...x.events, { type: "user", text: prompt, images: imgs.map((i) => i.url), oneShot, skill }] }));
    try {
      const summary = await invoke<RunSummary>("run_session", { prompt: fullPrompt, path, backend: s.backend, apply, skipPermissions: yolo, model: s.model.trim() || null, sessionId: s.id, onEvent: ch });
      update(s.id, (x) => ({ ...x, busy: false, summary }));
    } catch (e) {
      update(s.id, (x) => ({ ...x, busy: false, events: [...x.events, { type: "error", message: String(e) }] }));
    }
  }

  function cancel(s: Session) {
    invoke("cancel_session", { sessionId: s.id }).catch(() => {});
  }

  function stopRecording() {
    invoke<string[]>("stop_recording").then((steps) => { setRecording(false); setRecordSteps(steps); }).catch(() => setRecording(false));
  }
  function saveRecording(name: string, edited?: string[]) {
    const steps = (edited ?? recordSteps ?? []).map((s) => s.trim()).filter(Boolean);
    invoke<string>("save_recorded_skill", { path: active.path, name: name.trim() || "Recorded workflow", steps })
      .then((n) => { setRecordSteps(null); setScreen("chat"); setSkillsVersion((v) => v + 1); update(active.id, (s) => ({ ...s, events: [...s.events, { type: "message", text: `Saved recorded skill **${n}** (${steps.length} step${steps.length === 1 ? "" : "s"}) — it's in your skills (type / to use it) and synced to Claude Code.` }] })); })
      .catch((e) => update(active.id, (s) => ({ ...s, events: [...s.events, { type: "error", message: `Couldn't save skill: ${String(e)}` }] })));
  }
  function command(id: string) {
    if (id.startsWith("soon:")) {
      const c = id.slice(5);
      update(active.id, (s) => ({ ...s, events: [...s.events, { type: "message", text: `\`${c}\` is coming soon.` }] }));
    } else if (id === "clear") update(active.id, (s) => ({ ...s, events: [], summary: undefined }));
    else if (id === "apply") update(active.id, (s) => ({ ...s, apply: true }));
    else if (id === "plan") update(active.id, (s) => ({ ...s, apply: false, yolo: false }));
    else if (id === "help") update(active.id, (s) => ({ ...s, events: [...s.events, { type: "message", text: SLASH_HELP }] }));
    else if (id === "skill" || id.startsWith("skill:")) {
      const want = id.startsWith("skill:") ? id.slice(6).trim().toLowerCase() : "";
      const sid = active.id;
      invoke<SkillInfo[]>("skills", { path: active.path }).then((sk) => {
        if (!sk.length) { update(sid, (s) => ({ ...s, events: [...s.events, { type: "message", text: "No learned skills for this repo yet — `/record` one, or install from the Hub." }] })); return; }
        if (want) {
          const hit = sk.find((x) => x.name.toLowerCase() === want) ?? sk.find((x) => x.name.toLowerCase().startsWith(want)) ?? sk.find((x) => x.name.toLowerCase().includes(want));
          if (hit) update(sid, (s) => ({ ...s, attachedSkill: hit.name, attachedSkillGui: hit.gui, events: [...s.events, { type: "message", text: `Attached **${hit.name}** — ${hit.gui ? "it will replay its recorded steps when you send" : "it will guide the next message"}.` }] }));
          else update(sid, (s) => ({ ...s, events: [...s.events, { type: "message", text: `No skill matching \`${want}\`. Available: ${sk.map((x) => `\`${x.name}\``).join(", ")}` }] }));
        } else {
          update(sid, (s) => ({ ...s, events: [...s.events, { type: "message", text: ["**Learned skills** — attach one with `/skill <name>`:", "", ...sk.map((x) => `- **${x.name}**${x.gui ? " · replays" : ""} · used ${x.uses}× — ${x.description || x.triggers.slice(0, 2).join(", ")}`)].join("\n") }] }));
        }
      }).catch((e) => update(sid, (s) => ({ ...s, events: [...s.events, { type: "error", message: String(e) }] })));
    }
    else if (id === "mcp" || id.startsWith("mcp:")) {
      const want = id.startsWith("mcp:") ? id.slice(4).trim().toLowerCase() : "";
      const sid = active.id;
      update(sid, (s) => ({ ...s, events: [...s.events, { type: "message", text: want ? `Checking MCP servers for \`${want}\`…` : "Checking connected MCP servers…" }] }));
      invoke<McpServer[]>("list_mcp").then((servers) => {
        if (!servers.length) { update(sid, (s) => ({ ...s, events: [...s.events, { type: "message", text: "No MCP servers registered with Claude Code — add some in **Settings → Tools (MCP)** or browse the library." }] })); return; }
        if (want) {
          const hit = servers.find((x) => x.name.toLowerCase() === want) ?? servers.find((x) => x.name.toLowerCase().startsWith(want));
          if (hit) update(sid, (s) => ({ ...s, draft: `Use the ${hit.name} MCP tools to `, events: [...s.events, { type: "message", text: `Targeting **${hit.name}**${hit.connected ? "" : " (currently not connected — it may fail)"} — finish the sentence in the composer and send.` }] }));
          else update(sid, (s) => ({ ...s, events: [...s.events, { type: "message", text: `No server matching \`${want}\`. Connected: ${servers.map((x) => `\`${x.name}\``).join(", ")}` }] }));
        } else {
          update(sid, (s) => ({ ...s, events: [...s.events, { type: "message", text: ["**MCP servers** (from Claude Code) — target one with `/mcp <server>`:", "", ...servers.map((x) => `- **${x.name}** — ${x.connected ? "✓ connected" : "✗ not connected"}`)].join("\n") }] }));
        }
      }).catch((e) => update(sid, (s) => ({ ...s, events: [...s.events, { type: "error", message: String(e) }] })));
    }
    else if (id === "brief") {
      // A structured brief (goal / acceptance / constraints / test) reliably gets
      // better results than a one-liner — scaffold it into the composer to fill in.
      const tpl = "Goal: \nAcceptance criteria: \nConstraints: \nTest command: ";
      update(active.id, (s) => ({ ...s, draft: tpl, events: [...s.events, { type: "message", text: "Drafted a task brief in the composer — fill in the goal, how you'll know it's done, any constraints, and the test command, then send. A structured brief steers the agent far better than a one-line ask." }] }));
    }
    else if (id === "record") {
      if (recording || recordSteps) stopRecording();
      else invoke("start_recording").then(() => setRecording(true)).catch((e) => update(active.id, (s) => ({ ...s, events: [...s.events, { type: "error", message: `Couldn't start recording: ${String(e)}` }] })));
    }
  }

  return (
    <div style={{ ...themeStyle(effTheme), ...(customVars as React.CSSProperties), colorScheme: effTheme === "dark" ? "dark" : "light", height: "100vh", display: "flex", flexDirection: "column", background: "var(--bg)", color: "var(--fg)", overflow: "hidden" }}>
      {!prefs.onboarded && <Onboarding backends={backends} hasFolder={!!active?.path?.trim()} onPickFolder={async () => { const p = await invoke<string | null>("pick_folder"); if (p) update(active.id, (s) => ({ ...s, path: p })); }} onRecheck={recheck} onDone={() => savePrefs({ onboarded: true })} />}
      {updateInfo && <UpdateBanner info={updateInfo} busy={updateBusy} onUpdate={runUpdate} onDismiss={() => setUpdateInfo(null)} />}
      {whatsNew && <WhatsNewModal version={whatsNew.version} sections={whatsNew.sections} onClose={() => { savePrefs({ whats_new_seen: whatsNew.version }); setWhatsNew(null); }} />}
      {doctor && !anyBackend && prefs.onboarded && <SetupBanner onOpenSettings={() => setScreen("settings")} />}
      {(recording || recordSteps) && <RecordBar recording={recording} steps={recordSteps} onStop={stopRecording} onSave={saveRecording} onDiscard={() => setRecordSteps(null)} />}
      <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
        {navOpen && (
          <Rail
            screen={screen} sessions={sessions} activeId={active.id} theme={theme} claudeVersion={doctor?.claude_version ?? null}
            onNav={navTo} onNew={addSession} onOpen={openSession} onClose={closeSession} onTheme={setTheme} onCollapse={() => setNavOpen(false)}
            onPin={(id) => update(id, (s) => ({ ...s, pinned: !s.pinned }))} onProject={(id, project) => update(id, (s) => ({ ...s, project }))}
            onProjectRename={(from, to) => setSessions((prev) => prev.map((s) => (s.project === from ? { ...s, project: to } : s)))}
            onProjectDelete={(name) => setSessions((prev) => prev.map((s) => (s.project === name ? { ...s, project: undefined } : s)))}
            onImport={(partial) => { const ns = { ...newSession(homeRef.current), ...partial, id: uid(), title: `${partial.title ?? "Imported chat"}`, updatedAt: Date.now() } as Session; setSessions((prev) => [ns, ...prev]); setActiveId(ns.id); setScreen("chat"); }}
          />
        )}
        <div style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column", position: "relative" }}>
          {screen === "chat" && tiles && (
            <TileView
              node={tiles}
              sessions={sessions}
              activeId={activeId}
              onRatio={(splitId, ratio) => setTiles((t) => (t ? tileSetRatio(t, splitId, ratio) : t))}
              renderChat={(sess) => (
                <Chat key={sess.id} cleanDefault={prefs.clean_output} session={sess} backends={backends} models={models} claudeAvailable={claudeAvailable} navCollapsed tiled onShowNav={() => setNavOpen(true)} skillsVersion={skillsVersion} personalAgent={!!sess.personalAgent} dark={effTheme === "dark"} onPatch={(fn) => update(sess.id, fn)} onSend={() => send(sess)} onCancel={() => cancel(sess)} onCommand={command} onSplit={(dir) => splitPane(null, dir)} />
              )}
              onFocus={(sid) => setActiveId(sid)}
              onPick={(leafId, sid) => setTiles((t) => (t ? tileSetSession(t, leafId, sid) : t))}
              onSplit={(leafId, dir) => splitPane(leafId, dir)}
              onClose={(leafId) => closePane(leafId)}
            />
          )}
          {(screen === "agent" || (screen === "chat" && !tiles)) && <Chat key={active.id} cleanDefault={prefs.clean_output} session={active} backends={backends} models={models} claudeAvailable={claudeAvailable} navCollapsed={!navOpen} onShowNav={() => setNavOpen(true)} skillsVersion={skillsVersion} personalAgent={!!active.personalAgent} dark={effTheme === "dark"} onPatch={(fn) => update(active.id, fn)} onSend={() => send(active)} onCancel={() => cancel(active)} onCommand={command} onSplit={active.personalAgent ? undefined : (dir) => splitPane(null, dir)} />}
          {screen !== "chat" && screen !== "agent" && !navOpen && (
            <button onClick={() => setNavOpen(true)} className="icon-btn" title="Show sidebar"
              style={{ position: "absolute", top: 12, left: 12, zIndex: 30, width: 32, height: 32, borderRadius: 8, border: "1px solid var(--bd)", background: "var(--panel)", color: "var(--fg2)", cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center" }}><Icon name="panelLeft" size={17} /></button>
          )}
          {screen === "marketplace" && <Suspense fallback={<div style={{ maxWidth: 980, margin: "0 auto", padding: 40 }}><div className="lectern-skel" style={{ height: 34, width: 220, marginBottom: 18 }} /><div className="lectern-skel" style={{ height: 120 }} /></div>}><Marketplace path={active.path} onRefine={(text) => { update(active.id, (s2) => ({ ...s2, draft: text })); navTo("chat"); }} /></Suspense>}
          {screen === "brain" && <Suspense fallback={<div style={{ maxWidth: 980, margin: "0 auto", padding: 40 }}><div className="lectern-skel" style={{ height: 34, width: 220, marginBottom: 18 }} /><div className="lectern-skel" style={{ height: 260 }} /></div>}><Brain path={active.path} /></Suspense>}
          {screen === "usage" && <Suspense fallback={null}><Usage /></Suspense>}
          {screen === "connect" && <Suspense fallback={null}><Connect mcp={mcp} onRefresh={loadMcp} onBack={() => navTo("settings")} /></Suspense>}
          {screen === "schedule" && <Suspense fallback={<div style={{ maxWidth: 980, margin: "0 auto", padding: 40 }}><div className="lectern-skel" style={{ height: 30, width: 180, marginBottom: 20 }} /><div className="lectern-skel" style={{ height: 140 }} /></div>}><Schedule path={active.path} /></Suspense>}
          {screen === "settings" && <Suspense fallback={<div style={{ maxWidth: 680, margin: "0 auto", padding: 44 }}><div className="lectern-skel" style={{ height: 30, width: 160, marginBottom: 24 }} /><div className="lectern-skel" style={{ height: 180 }} /></div>}><Settings onBrowse={() => navTo("connect")} backends={backends} models={models} prefs={prefs} mcp={mcp} onMcp={loadMcp} onPrefs={savePrefs} onRecheck={recheck} /></Suspense>}
          {screen === "profile" && <Profile onSignOutHint={() => {}} />}
        </div>
      </div>
    </div>
  );
}

type ChangeSection = { name: string; items: string[] };

// Extract the current version's changelog section from the repo's CHANGELOG.md — the same
// source the website and GitHub releases use. Best-effort: offline just skips the panel.
async function fetchWhatsNew(version: string): Promise<ChangeSection[] | null> {
  try {
    const res = await fetch("https://raw.githubusercontent.com/ShrimpScript/lectern/main/CHANGELOG.md");
    if (!res.ok) return null;
    return parseVersionSections(await res.text(), version);
  } catch {
    return null;
  }
}

function parseVersionSections(md: string, version: string): ChangeSection[] {
  const esc = version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const head = new RegExp("^##\\s+\\[" + esc + "\\]");
  const sections: ChangeSection[] = [];
  let cur: ChangeSection | null = null;
  let grab = false;
  for (const raw of md.split("\n")) {
    const line = raw.replace(/\s+$/, "");
    if (head.test(line)) { grab = true; continue; }
    if (!grab) continue;
    if (/^##\s+/.test(line) || /^\[[^\]]+\]:\s/.test(line)) break; // next version / link refs
    const sec = line.match(/^###\s+(.+)/);
    if (sec) { cur = { name: sec[1].trim(), items: [] }; sections.push(cur); continue; }
    const bullet = line.match(/^-\s+(.+)/);
    if (bullet) { if (!cur) { cur = { name: "", items: [] }; sections.push(cur); } cur.items.push(bullet[1]); continue; }
    if (/^\s+-\s+/.test(line)) continue; // sub-bullet detail — omit from the headline panel
    const cont = line.match(/^\s{2,}(\S.+)/);
    if (cont && cur?.items.length) cur.items[cur.items.length - 1] += " " + cont[1];
  }
  return sections.filter((s) => s.items.length);
}

// You just updated — here's what changed. A dismissible, on-brand card shown once per
// version. Reduced-motion-safe (the fade is disabled under prefers-reduced-motion).
function WhatsNewModal({ version, sections, onClose }: { version: string; sections: ChangeSection[]; onClose: () => void }) {
  return (
    <div onClick={onClose} className="lectern-fadein" style={{ position: "fixed", inset: 0, zIndex: 200, background: "rgba(0,0,0,0.45)", display: "flex", alignItems: "center", justifyContent: "center", padding: 20 }}>
      <div onClick={(e) => e.stopPropagation()} style={{ background: "var(--panel)", border: "1px solid var(--bd)", borderRadius: 14, maxWidth: 540, width: "100%", maxHeight: "80vh", overflowY: "auto", padding: "28px 30px", boxShadow: "0 24px 70px rgba(0,0,0,0.42)" }}>
        <div className="mono" style={{ fontSize: 11, color: "var(--fg3)", letterSpacing: "0.14em", textTransform: "uppercase" }}>What&apos;s new</div>
        <h2 style={{ margin: "6px 0 20px", fontSize: 25, fontWeight: 800, letterSpacing: "-0.02em" }}>Lectern {version}</h2>
        {sections.map((s, i) => (
          <div key={i} style={{ marginBottom: 18 }}>
            {s.name && <div className="mono" style={{ fontSize: 11, color: "var(--fg3)", textTransform: "uppercase", letterSpacing: "0.08em", marginBottom: 9 }}>{s.name}</div>}
            <ul style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 9 }}>
              {s.items.map((it, j) => <li key={j} style={{ fontSize: 14, lineHeight: 1.5, color: "var(--fg2)" }}>{inlineMd(it, `wn${i}-${j}`)}</li>)}
            </ul>
          </div>
        ))}
        <button onClick={onClose} style={{ marginTop: 4, height: 34, padding: "0 18px", background: "var(--btn)", color: "var(--btnfg)", border: "none", borderRadius: 8, fontSize: 13, fontWeight: 700, cursor: "pointer", fontFamily: "inherit" }}>Got it</button>
      </div>
    </div>
  );
}

// A newer signed release is available. Calm, monochrome, no animation — one line with
// optional release notes and the two actions.
function UpdateBanner({ info, busy, onUpdate, onDismiss }: { info: { version: string; notes: string }; busy: boolean; onUpdate: () => void; onDismiss: () => void }) {
  const [showNotes, setShowNotes] = useState(false);
  // Release notes come from the manifest (Markdown); drop heading markers so the banner
  // reads cleanly whatever the source formatting.
  const notes = info.notes.replace(/^#{1,6}\s+/gm, "").trim();
  const btn = (primary: boolean): React.CSSProperties => ({ height: 24, padding: "0 11px", fontSize: 11.5, fontWeight: 700, border: primary ? "none" : "1px solid var(--bd)", background: primary ? "var(--btn)" : "transparent", color: primary ? "var(--btnfg)" : "var(--fg2)", borderRadius: 7, cursor: busy ? "default" : "pointer", fontFamily: "inherit", flexShrink: 0, opacity: busy ? 0.6 : 1 });
  return (
    <div className="mono" style={{ background: "var(--panel)", borderBottom: "1px solid var(--bd)", color: "var(--fg2)", fontSize: 12, lineHeight: 1.5, padding: "9px 16px", flexShrink: 0, display: "flex", flexDirection: "column", gap: showNotes ? 8 : 0 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: 10, flexWrap: "wrap" }}>
        <span>Lectern {info.version} is available.</span>
        {notes && <button onClick={() => setShowNotes((v) => !v)} style={{ border: "none", background: "transparent", color: "var(--fg2)", cursor: "pointer", font: "inherit", textDecoration: "underline", textUnderlineOffset: 2, padding: 0 }}>{showNotes ? "Hide notes" : "What's new"}</button>}
        <button disabled={busy} onClick={onUpdate} style={btn(true)}>{busy ? "Updating…" : "Restart & update"}</button>
        <button disabled={busy} onClick={onDismiss} style={btn(false)}>Later</button>
      </div>
      {showNotes && notes && <div style={{ maxWidth: 720, margin: "0 auto", whiteSpace: "pre-wrap", fontSize: 11.5, maxHeight: 160, overflowY: "auto" }}>{notes}</div>}
    </div>
  );
}

function SetupBanner({ onOpenSettings }: { onOpenSettings: () => void }) {
  return (
    <div className="mono" style={{ background: "var(--panel)", borderBottom: "1px solid var(--bd)", color: "var(--fg2)", fontSize: 12, lineHeight: 1.5, padding: "9px 16px", textAlign: "center", flexShrink: 0, display: "flex", alignItems: "center", justifyContent: "center", gap: 10, flexWrap: "wrap" }}>
      <span><span style={{ color: WARN }}>●</span> No agent provider connected. OpenCode&apos;s free models need no key, or connect Claude Code / Antigravity.</span>
      <button onClick={onOpenSettings} style={{ height: 24, padding: "0 11px", fontSize: 11.5, fontWeight: 700, color: "var(--btnfg)", background: "var(--btn)", border: "none", borderRadius: 7, cursor: "pointer", fontFamily: "inherit", flexShrink: 0 }}>Set one up →</button>
    </div>
  );
}

// First-run onboarding — one setup checklist (replaces scattered banners): connect the agent
// CLIs you have, pick a project, start. Shown until the user completes or skips it.
function RecordBar({ recording, steps, onStop, onSave, onDiscard }: { recording: boolean; steps: string[] | null; onStop: () => void; onSave: (name: string, steps: string[]) => void; onDiscard: () => void }) {
  const [name, setName] = useState("");
  // Review before it counts: the captured steps, editable — fix phrasing, drop noise.
  const [edit, setEdit] = useState<string[] | null>(null);
  const rows = edit ?? steps ?? [];
  const bar: React.CSSProperties = { display: "flex", alignItems: "center", gap: 12, flexShrink: 0, padding: "9px 16px", borderBottom: "1px solid var(--bd)", background: "var(--panel)", fontSize: 13 };
  const btn = (bg: string, fg: string): React.CSSProperties => ({ height: 30, padding: "0 14px", borderRadius: 8, border: "none", background: bg, color: fg, fontSize: 12.5, fontWeight: 700, cursor: "pointer", fontFamily: "inherit", flexShrink: 0 });
  if (recording) {
    return (
      <div style={bar}>
        <span style={{ width: 9, height: 9, borderRadius: "50%", background: DANGER, flexShrink: 0 }} />
        <span style={{ color: "var(--fg)" }}>Recording your screen actions — perform your workflow, then stop.</span>
        <button onClick={onStop} style={{ ...btn(DANGER, "#fff"), marginLeft: "auto" }}>Stop &amp; save</button>
      </div>
    );
  }
  return (
    <div style={{ flexShrink: 0, borderBottom: "1px solid var(--bd)", background: "var(--panel)" }}>
      <div style={{ ...bar, borderBottom: "none" }}>
        <span style={{ color: "var(--fg)", flexShrink: 0 }}>Captured <b>{rows.length}</b> step{rows.length === 1 ? "" : "s"} — review, then name this skill:</span>
        <input autoFocus value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") onSave(name, rows); }} placeholder="e.g. Open the deploy dashboard" className="mono" style={{ ...ctrl, flex: 1, minWidth: 0, height: 30 }} />
        <button onClick={() => onSave(name, rows)} style={btn("var(--btn)", "var(--btnfg)")}>Save skill</button>
        <button onClick={onDiscard} style={{ height: 30, padding: "0 12px", borderRadius: 8, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg2)", fontSize: 12.5, cursor: "pointer", fontFamily: "inherit", flexShrink: 0 }}>Discard</button>
      </div>
      {rows.length > 0 && (
        <div style={{ maxHeight: 180, overflow: "auto", padding: "0 16px 10px", display: "flex", flexDirection: "column", gap: 4 }}>
          {rows.map((st, i) => (
            <div key={i} style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--fg3)", width: 18, textAlign: "right", flexShrink: 0 }}>{i + 1}</span>
              <input
                value={st}
                onChange={(e) => { const next = [...rows]; next[i] = e.target.value; setEdit(next); }}
                className="mono"
                style={{ ...ctrl, flex: 1, minWidth: 0, height: 26, fontSize: 11.5 }}
              />
              <button onClick={() => setEdit(rows.filter((_, j) => j !== i))} title="Remove this step"
                style={{ width: 22, height: 22, borderRadius: 6, border: "none", background: "transparent", color: "var(--fg3)", cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center", padding: 0 }}>
                <Icon name="x" size={12} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function relTime(ts?: number): string {
  if (!ts) return "";
  const s = Math.max(0, Math.floor((Date.now() - ts) / 1000));
  if (s < 45) return "now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

// The sidebar's chat list: search + Pinned + Projects (folders) + recent — with relative
// timestamps and a per-row menu (pin / move to project / close).
/* Mission D3 — chat export/import. JSON is the re-importable format (versioned
   envelope); Markdown is the readable one. Import happens via a plain file input
   (webview-native, no dialog plugin). */
function chatToJson(s: Session): string {
  return JSON.stringify({ lectern_chat: 1, title: s.title, project: s.project ?? null, backend: s.backend, model: s.model, exportedAt: new Date().toISOString(), events: s.events }, null, 2);
}
function chatToMarkdown(s: Session): string {
  const lines: string[] = [`# ${s.title}`, "", `> Lectern chat · ${s.backend}${s.model ? ` · ${s.model}` : ""} · exported ${new Date().toISOString().slice(0, 10)}`, ""];
  for (const ev of s.events) {
    switch (ev.type) {
      case "user": lines.push("## You", "", String((ev as { text?: string }).text ?? ""), ""); break;
      case "message": lines.push("## Lectern", "", String((ev as { text?: string }).text ?? ""), ""); break;
      case "thought": lines.push(`- 🧠 ${(ev as { summary: string }).summary}`); break;
      case "skill_applied": lines.push(`- ✦ skill: ${(ev as { name: string }).name}`); break;
      case "model_routed": lines.push(`- ⇄ routed to ${(ev as { model: string }).model}`); break;
      case "plan": lines.push("**Plan**", ...((ev as { steps: { done: boolean; text: string }[] }).steps ?? []).map((st) => `- [${st.done ? "x" : " "}] ${st.text}`), ""); break;
      case "file_edit": { const fe = ev as { path: string; added: number; removed: number }; lines.push(`\n\`\`\`\n✎ ${fe.path}  +${fe.added} −${fe.removed}\n\`\`\`\n`); break; }
      case "terminal": { const t = ev as { command: string; output: string }; lines.push("```bash", `$ ${t.command}`, ...(t.output ? t.output.split("\n").slice(0, 12) : []), "```", ""); break; }
      case "usage": { const u = ev as { input_tokens: number; output_tokens: number }; lines.push("", `> ${u.input_tokens} in / ${u.output_tokens} out tokens`); break; }
      default: break;
    }
  }
  return lines.join("\n");
}
function parseChatImport(text: string): Partial<Session> | null {
  try {
    const d = JSON.parse(text);
    if (d?.lectern_chat !== 1 || !Array.isArray(d.events)) return null;
    return { title: String(d.title ?? "Imported chat"), project: d.project ?? undefined, backend: String(d.backend ?? "auto"), model: String(d.model ?? ""), events: d.events as Ev[] };
  } catch { return null; }
}

function SessionList({ sessions, activeId, screen, onOpen, onClose, onPin, onProject, onProjectRename, onProjectDelete, onImport }: {
  sessions: Session[]; activeId: string; screen: Screen; onOpen: (id: string) => void; onClose: (id: string) => void; onPin: (id: string) => void; onProject: (id: string, project?: string) => void; onProjectRename: (from: string, to: string) => void; onProjectDelete: (name: string) => void; onImport: (s: Partial<Session>) => void;
}) {
  const [notice, setNotice] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement | null>(null);
  const [dragOver, setDragOver] = useState<string | null>(null); // project name or "__root__"
  const [projMenu, setProjMenu] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameVal, setRenameVal] = useState("");
  const [q, setQ] = useState("");
  const [menu, setMenu] = useState<string | null>(null);
  const [newProjFor, setNewProjFor] = useState<string | null>(null);
  const [newProjName, setNewProjName] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  // Any open dropdown dies on click-anywhere-else or Esc (QoL bug: menus stuck open).
  useEffect(() => {
    if (menu === null && projMenu === null) return;
    const dismiss = () => { setMenu(null); setProjMenu(null); setNewProjFor(null); };
    const onDown = (e: MouseEvent) => {
      if ((e.target as HTMLElement).closest?.("[data-menu-keep]")) return;
      dismiss();
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") dismiss(); };
    document.addEventListener("mousedown", onDown, true);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", onDown, true);
      document.removeEventListener("keydown", onKey, true);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [menu, projMenu]);
  const toggleProject = (p: string) => setCollapsed((c) => { const n = new Set(c); if (n.has(p)) n.delete(p); else n.add(p); return n; });
  const chats = sessions.filter((s) => !s.personalAgent);
  const allProjects = [...new Set(chats.filter((s) => s.project).map((s) => s.project!))].sort();
  const matched = chats.filter((s) => !q || `${s.title} ${s.project ?? ""}`.toLowerCase().includes(q.toLowerCase()));
  const sorted = [...matched].sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
  const pinned = sorted.filter((s) => s.pinned);
  const rest = sorted.filter((s) => !s.pinned);
  const projects = [...new Set(rest.filter((s) => s.project).map((s) => s.project!))].sort();
  const ungrouped = rest.filter((s) => !s.project);
  const grpLabel: React.CSSProperties = { padding: "11px 10px 4px", fontSize: 11, fontWeight: 600, color: "var(--fg3)", display: "flex", alignItems: "center", gap: 6 };
  const menuItem: React.CSSProperties = { padding: "6px 9px", borderRadius: 6, cursor: "pointer", whiteSpace: "nowrap" };

  const row = (s: Session) => {
    const on = s.id === activeId && screen === "chat";
    return (
      <div key={s.id} onClick={() => onOpen(s.id)} draggable
        onDragStart={(e) => { e.dataTransfer.setData("text/plain", s.id); e.dataTransfer.effectAllowed = "move"; }}
        onContextMenu={(e) => { e.preventDefault(); setNewProjFor(null); setMenu(menu === s.id ? null : s.id); }}
        className="lectern-row" style={{ position: "relative", display: "flex", alignItems: "center", gap: 8, height: 32, padding: "0 6px 0 10px", borderRadius: 8, fontSize: 13, cursor: "pointer", color: on ? "var(--fg)" : "var(--fg2)", background: on ? "var(--hov)" : "transparent" }}>
        <span style={{ width: 6, height: 6, borderRadius: "50%", flexShrink: 0, background: s.busy ? ACCENT : "var(--bd)" }} />
        {s.pinned && (
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden style={{ flexShrink: 0, color: "var(--fg3)", transform: "rotate(38deg)" }}>
            <path d="M12 17v5M7 9.5 5.5 11c-.6.6-.2 1.6.6 1.7l11.7 1.6c.9.1 1.5-.9 1-1.6l-1.3-1.9M9 4h6l-.6 6.2c0 .5.2 1 .5 1.3l2.1 2.1M9.6 10.2 7.4 12.4" />
          </svg>
        )}
        <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{s.title}</span>
        <span className="mono" style={{ fontSize: 10, color: "var(--fg3)", flexShrink: 0 }}>{relTime(s.updatedAt)}</span>
        <span data-menu-keep onClick={(e) => { e.stopPropagation(); setNewProjFor(null); setMenu(menu === s.id ? null : s.id); }} title="More" style={{ color: "var(--fg3)", lineHeight: 1, padding: "0 3px", flexShrink: 0, display: "inline-flex" }}><Icon name="more" size={15} /></span>
        {menu === s.id && (
          <div data-menu-keep onClick={(e) => e.stopPropagation()} className="lectern-pop" style={{ position: "absolute", top: "calc(100% - 2px)", right: 6, zIndex: 40, minWidth: 178, background: "var(--panel)", border: "1px solid var(--bd)", borderRadius: 10, boxShadow: "0 14px 40px -12px rgba(0,0,0,.3)", padding: 5, fontSize: 12.5, color: "var(--fg2)" }}>
            <div className="lectern-row" style={menuItem} onClick={() => { onPin(s.id); setMenu(null); }}>{s.pinned ? "Unpin" : "Pin"}</div>
            <div className="lectern-row" style={menuItem} onClick={() => { const fn = `lectern-${s.title.slice(0, 24)}-${new Date().toISOString().slice(0, 10)}.json`; invoke<string>("save_chat_export", { filename: fn, content: chatToJson(s) }).then((p2) => { setNotice(`Saved ${p2}`); setTimeout(() => setNotice(null), 4000); }).catch(() => {}); setMenu(null); }}>Export as JSON</div>
            <div className="lectern-row" style={menuItem} onClick={() => { const fn = `lectern-${s.title.slice(0, 24)}-${new Date().toISOString().slice(0, 10)}.md`; invoke<string>("save_chat_export", { filename: fn, content: chatToMarkdown(s) }).then((p2) => { setNotice(`Saved ${p2}`); setTimeout(() => setNotice(null), 4000); }).catch(() => {}); setMenu(null); }}>Export as Markdown</div>
            {allProjects.filter((p) => p !== s.project).map((p) => (
              <div key={p} className="lectern-row" style={menuItem} onClick={() => { onProject(s.id, p); setMenu(null); }}>Move to “{p}”</div>
            ))}
            {s.project && <div className="lectern-row" style={menuItem} onClick={() => { onProject(s.id, undefined); setMenu(null); }}>Remove from project</div>}
            {newProjFor === s.id ? (
              <div style={{ display: "flex", gap: 4, padding: "4px 5px" }}>
                <input autoFocus value={newProjName} onChange={(e) => setNewProjName(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter" && newProjName.trim()) { onProject(s.id, newProjName.trim()); setMenu(null); setNewProjFor(null); setNewProjName(""); } if (e.key === "Escape") setNewProjFor(null); }}
                  placeholder="Project name" spellCheck={false} style={{ flex: 1, minWidth: 0, height: 26, borderRadius: 6, border: "1px solid var(--bd)", background: "var(--bg)", color: "var(--fg)", fontSize: 12, padding: "0 7px", outline: "none", fontFamily: "inherit" }} />
              </div>
            ) : (
              <div className="lectern-row" style={menuItem} onClick={() => { setNewProjFor(s.id); setNewProjName(""); }}>New project…</div>
            )}
            <div className="lectern-row" style={{ ...menuItem, color: DANGER }} onClick={() => { onClose(s.id); setMenu(null); }}>Close chat</div>
          </div>
        )}
      </div>
    );
  };

  return (
    <>
      <div style={{ padding: "6px 10px 4px", flexShrink: 0, display: "flex", gap: 6, alignItems: "center" }}>
        <input value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => { if (e.key === "Escape") setQ(""); }} placeholder="Search chats" spellCheck={false}
          style={{ flex: 1, minWidth: 0, height: 30, borderRadius: 9, border: "1px solid var(--bd)", background: "var(--bg)", color: "var(--fg)", fontSize: 12.5, padding: "0 11px", outline: "none", fontFamily: "inherit" }} />
        <button onClick={() => fileRef.current?.click()} title="Import a chat (.json exported from Lectern)" style={{ flexShrink: 0, width: 30, height: 30, borderRadius: 8, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg3)", cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center" }}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden><path d="M12 15V4m0 0 4 4m-4-4L8 8M4 16v3a1.5 1.5 0 0 0 1.5 1.5h13A1.5 1.5 0 0 0 20 19v-3" /></svg>
        </button>
        <input ref={fileRef} type="file" accept=".json,application/json" style={{ display: "none" }} onChange={(e) => {
          const f = e.target.files?.[0]; if (!f) return;
          f.text().then((t) => { const parsed = parseChatImport(t); if (parsed) onImport(parsed); else { setNotice("Not a Lectern chat export."); setTimeout(() => setNotice(null), 3500); } });
          e.target.value = "";
        }} />
      </div>
      {notice && <div className="lectern-fadein" style={{ padding: "2px 12px 4px", fontSize: 10.5, color: "var(--fg3)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flexShrink: 0 }} title={notice}>{notice}</div>}
      <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "0 8px 8px", display: "flex", flexDirection: "column", gap: 1 }}>
        {pinned.length > 0 && <div style={grpLabel}><svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden style={{ transform: "rotate(38deg)" }}><path d="M12 17v5M7 9.5 5.5 11c-.6.6-.2 1.6.6 1.7l11.7 1.6c.9.1 1.5-.9 1-1.6l-1.3-1.9M9 4h6l-.6 6.2c0 .5.2 1 .5 1.3l2.1 2.1M9.6 10.2 7.4 12.4" /></svg> Pinned</div>}
        {pinned.map(row)}
        {projects.map((p) => {
          const isCol = collapsed.has(p);
          const items = rest.filter((s) => s.project === p);
          return (
            <div key={p}>
              <div className="lectern-row"
                onDragOver={(e) => { e.preventDefault(); setDragOver(p); }}
                onDragLeave={() => setDragOver((d) => (d === p ? null : d))}
                onDrop={(e) => { e.preventDefault(); const id = e.dataTransfer.getData("text/plain"); if (id) onProject(id, p); setDragOver(null); }}
                onContextMenu={(e) => { e.preventDefault(); setProjMenu(projMenu === p ? null : p); }}
                style={{ ...grpLabel, cursor: "pointer", borderRadius: 6, position: "relative", background: dragOver === p ? "var(--hov)" : "transparent", outline: dragOver === p ? "1.5px dashed var(--bd2)" : "none" }} onClick={() => toggleProject(p)} title={isCol ? "Expand project" : "Collapse project"}>
                <span className="lectern-chev" style={{ display: "inline-block", transform: isCol ? "rotate(-90deg)" : "none", fontSize: 9, color: "var(--fg3)", lineHeight: 1 }}>▾</span>
                <Icon name="folder" size={13} />
                {renaming === p ? (
                  <input autoFocus value={renameVal} onClick={(e) => e.stopPropagation()} onChange={(e) => setRenameVal(e.target.value)}
                    onKeyDown={(e) => { if (e.key === "Enter" && renameVal.trim()) { onProjectRename(p, renameVal.trim()); setRenaming(null); } if (e.key === "Escape") setRenaming(null); }}
                    style={{ flex: 1, minWidth: 0, height: 20, borderRadius: 5, border: "1px solid var(--bd)", background: "var(--bg)", color: "var(--fg)", fontSize: 11, padding: "0 6px", outline: "none", fontFamily: "inherit" }} />
                ) : (
                  <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{p}</span>
                )}
                <span className="mono" style={{ fontSize: 10, color: "var(--fg3)", fontWeight: 400 }}>{items.length}</span>
                <span data-menu-keep onClick={(e) => { e.stopPropagation(); setProjMenu(projMenu === p ? null : p); }} title="Folder options" style={{ color: "var(--fg3)", lineHeight: 1, padding: "0 2px", display: "inline-flex" }}><Icon name="more" size={13} /></span>
                {projMenu === p && (
                  <div data-menu-keep onClick={(e) => e.stopPropagation()} className="lectern-pop" style={{ position: "absolute", top: "100%", right: 4, zIndex: 41, minWidth: 150, background: "var(--panel)", border: "1px solid var(--bd)", borderRadius: 10, boxShadow: "0 14px 40px -12px rgba(0,0,0,.3)", padding: 5, fontSize: 12.5, color: "var(--fg2)" }}>
                    <div className="lectern-row" style={menuItem} onClick={() => { setRenaming(p); setRenameVal(p); setProjMenu(null); }}>Rename folder</div>
                    <div className="lectern-row" style={menuItem} onClick={() => { onProjectDelete(p); setProjMenu(null); }}>Delete folder (keep chats)</div>
                  </div>
                )}
              </div>
              {!isCol && items.map(row)}
            </div>
          );
        })}
        {ungrouped.length > 0 && (pinned.length > 0 || projects.length > 0) && (
          <div style={{ ...grpLabel, background: dragOver === "__root__" ? "var(--hov)" : "transparent", borderRadius: 6, outline: dragOver === "__root__" ? "1.5px dashed var(--bd2)" : "none" }}
            onDragOver={(e) => { e.preventDefault(); setDragOver("__root__"); }}
            onDragLeave={() => setDragOver((d) => (d === "__root__" ? null : d))}
            onDrop={(e) => { e.preventDefault(); const id = e.dataTransfer.getData("text/plain"); if (id) onProject(id, undefined); setDragOver(null); }}>Chats</div>
        )}
        {ungrouped.map(row)}
        {matched.length === 0 && <div style={{ padding: "10px", fontSize: 12, color: "var(--fg3)" }}>No chats found.</div>}
      </div>
    </>
  );
}

function Rail({ screen, sessions, activeId, theme, claudeVersion, onNav, onNew, onOpen, onClose, onTheme, onCollapse, onPin, onProject, onProjectRename, onProjectDelete, onImport }: {
  screen: Screen; sessions: Session[]; activeId: string; theme: ThemeName; claudeVersion: string | null;
  onNav: (s: Screen) => void; onNew: () => void; onOpen: (id: string) => void; onClose: (id: string) => void; onTheme: (t: ThemeName) => void; onCollapse: () => void;
  onPin: (id: string) => void; onProject: (id: string, project?: string) => void; onProjectRename: (from: string, to: string) => void; onProjectDelete: (name: string) => void; onImport: (s: Partial<Session>) => void;
}) {
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const navItem = (n: { id: Screen; label: string; icon: string }) => {
    const on = screen === n.id;
    return (
      <div key={n.id} onClick={() => onNav(n.id)} className="icon-btn" style={{ height: 34, display: "flex", alignItems: "center", gap: 10, padding: "0 10px", borderRadius: 9, fontSize: 13.5, fontWeight: on ? 600 : 500, cursor: "pointer", color: on ? "var(--fg)" : "var(--fg2)", background: on ? "var(--hov)" : "transparent" }}>
        <span style={{ color: on ? "var(--fg)" : "var(--fg3)", display: "flex", flexShrink: 0 }}><Icon name={n.icon} size={17} /></span>
        {n.label}
      </div>
    );
  };
  return (
    <div style={{ width: 224, flexShrink: 0, borderRight: "1px solid var(--bd)", background: "var(--panel)", display: "flex", flexDirection: "column" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 9, height: 52, padding: "0 12px 0 16px", fontWeight: 800, fontSize: 16, letterSpacing: "-0.01em", flexShrink: 0 }}>
        <Logo /> Lectern
        <button onClick={onCollapse} className="icon-btn" title="Collapse sidebar" style={{ marginLeft: "auto", width: 28, height: 28, borderRadius: 7, border: "none", background: "transparent", color: "var(--fg2)", cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center" }}><Icon name="panelLeftClose" size={17} /></button>
      </div>

      <div style={{ padding: "2px 12px 8px", flexShrink: 0 }}>
        <button onClick={onNew} style={{ width: "100%", height: 36, borderRadius: 9, fontSize: 13, fontWeight: 600, cursor: "pointer", border: "none", background: "var(--btn)", color: "var(--btnfg)", fontFamily: "inherit", display: "flex", alignItems: "center", justifyContent: "center", gap: 7 }}><Icon name="newsession" size={15} /> New session</button>
      </div>

      <div style={{ padding: "4px 8px", display: "flex", flexDirection: "column", gap: 1, flexShrink: 0 }}>
        {NAV_PRIMARY.map(navItem)}
        <button onClick={() => setAdvancedOpen((o) => !o)} className="icon-btn" style={{ height: 30, marginTop: 4, display: "flex", alignItems: "center", gap: 8, padding: "0 10px", borderRadius: 9, fontSize: 11.5, fontWeight: 600, cursor: "pointer", color: "var(--fg3)", background: "transparent", border: "none", fontFamily: "inherit", textAlign: "left", width: "100%" }}>
          <span className="lectern-chev" style={{ fontSize: 10, opacity: 0.7, transform: advancedOpen ? "rotate(90deg)" : "none" }}>▸</span> Advanced
        </button>
        {advancedOpen && NAV_ADVANCED.map(navItem)}
      </div>

      {sessions.some((s) => !s.personalAgent) ? (
        <SessionList sessions={sessions} activeId={activeId} screen={screen} onOpen={onOpen} onClose={onClose} onPin={onPin} onProject={onProject} onProjectRename={onProjectRename} onProjectDelete={onProjectDelete} onImport={onImport} />
      ) : (
        <div style={{ flex: 1 }} />
      )}

      <div style={{ padding: "8px 12px 10px", borderTop: "1px solid var(--bd)", display: "flex", alignItems: "center", gap: 8, flexShrink: 0 }}>
        <span className="mono" title={claudeVersion ? "Claude Code" : "Claude Code not found"} style={{ flex: 1, display: "flex", alignItems: "center", gap: 7, fontSize: 11, color: "var(--fg3)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          <span style={{ color: claudeVersion ? ACCENT : "var(--fg3)", flexShrink: 0 }}>{claudeVersion ? "●" : "○"}</span>
          {claudeVersion ? claudeVersion.replace(" (Claude Code)", "") : "not found"}
        </span>
        <button onClick={() => onTheme(theme === "dark" ? "light" : "dark")} title="Toggle light / dark" className="icon-btn"
          style={{ width: 28, height: 28, flexShrink: 0, borderRadius: 7, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg2)", cursor: "pointer", fontFamily: "inherit", display: "inline-flex", alignItems: "center", justifyContent: "center" }}>{theme === "dark" ? <MoonIcon /> : <SunIcon />}</button>
      </div>
    </div>
  );
}


// ── Chat (3-pane) ────────────────────────────────────────────────────────────
/* Recursive tile renderer: splits are flex halves with a hairline divider;
   each leaf = pane header (session picker · split · close) + a full Chat. */
function TileView({ node, sessions, activeId, renderChat, onFocus, onPick, onSplit, onClose, onRatio }: {
  node: TileNode;
  sessions: Session[];
  activeId: string;
  renderChat: (s: Session) => React.ReactNode;
  onFocus: (sessionId: string) => void;
  onPick: (leafId: string, sessionId: string) => void;
  onSplit: (leafId: string, dir: "row" | "col") => void;
  onClose: (leafId: string) => void;
  onRatio?: (splitId: string, ratio: number) => void;
}) {
  if (node.kind === "split") {
    const ratio = node.ratio ?? 0.5;
    const pass = { sessions, activeId, renderChat, onFocus, onPick, onSplit, onClose, onRatio };
    // drag the divider → live ratio from pointer position within the container
    const startDrag = (e: React.MouseEvent<HTMLDivElement>) => {
      e.preventDefault();
      const box = (e.currentTarget.parentElement as HTMLDivElement).getBoundingClientRect();
      let raf = 0;
      const move = (ev: MouseEvent) => {
        if (raf) return; // coalesce to one ratio update per frame
        raf = requestAnimationFrame(() => {
          raf = 0;
          const r = node.dir === "row" ? (ev.clientX - box.left) / box.width : (ev.clientY - box.top) / box.height;
          onRatio?.(node.id, r);
        });
      };
      const up = () => { window.removeEventListener("mousemove", move); window.removeEventListener("mouseup", up); document.body.style.cursor = ""; };
      window.addEventListener("mousemove", move);
      window.addEventListener("mouseup", up);
      document.body.style.cursor = node.dir === "row" ? "col-resize" : "row-resize";
    };
    return (
      <div style={{ display: "flex", flexDirection: node.dir === "row" ? "row" : "column", flex: 1, minWidth: 0, minHeight: 0 }}>
        <div style={{ flex: `${ratio} 1 0%`, minWidth: 0, minHeight: 0, display: "flex" }}>
          <TileView node={node.a} {...pass} />
        </div>
        <div onMouseDown={startDrag}
          style={{ flexShrink: 0, zIndex: 5, background: "var(--bd)", cursor: node.dir === "row" ? "col-resize" : "row-resize",
            width: node.dir === "row" ? 6 : undefined, height: node.dir === "col" ? 6 : undefined,
            margin: node.dir === "row" ? "0 -2.5px" : "-2.5px 0", backgroundClip: "content-box",
            padding: node.dir === "row" ? "0 2.5px" : "2.5px 0" }} />
        <div style={{ flex: `${1 - ratio} 1 0%`, minWidth: 0, minHeight: 0, display: "flex" }}>
          <TileView node={node.b} {...pass} />
        </div>
      </div>
    );
  }
  const sess = sessions.find((s) => s.id === node.sessionId && !s.personalAgent);
  const chats = sessions.filter((s) => !s.personalAgent);
  const on = node.sessionId === activeId;
  const btn: React.CSSProperties = { width: 22, height: 22, display: "inline-flex", alignItems: "center", justifyContent: "center", border: "none", background: "transparent", color: "var(--fg3)", cursor: "pointer", borderRadius: 5, padding: 0 };
  return (
    <div onMouseDown={() => onFocus(node.sessionId)}
      style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column", outline: on ? "1.5px solid var(--bd2)" : "none", outlineOffset: -1 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 4, height: 30, padding: "0 6px 0 8px", borderBottom: "1px solid var(--bd)", background: "var(--panel)", flexShrink: 0 }}>
        <NiceSelect
          value={node.sessionId}
          minWidth={120}
          items={chats.map((c) => ({ id: c.id, label: c.title }))}
          onPick={(sid) => onPick(node.id, sid)}
        />
        <span style={{ flex: 1 }} />
        <button title="Split right" style={btn} onClick={() => onSplit(node.id, "row")}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden><rect x="3" y="4" width="18" height="16" rx="2.5" /><path d="M12 4v16" /></svg>
        </button>
        <button title="Split down" style={btn} onClick={() => onSplit(node.id, "col")}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden><rect x="3" y="4" width="18" height="16" rx="2.5" /><path d="M3 12h18" /></svg>
        </button>
        <button title="Close pane" style={btn} onClick={() => onClose(node.id)}>
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden><path d="m6 6 12 12M18 6 6 18" /></svg>
        </button>
      </div>
      <div style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column", position: "relative" }}>
        {sess ? renderChat(sess) : <div style={{ padding: 20, fontSize: 12.5, color: "var(--fg3)" }}>Session gone — pick another above.</div>}
      </div>
    </div>
  );
}

/* Mission D1 — clean output: machinery rows (thoughts, tools, routing, terminals)
   collapse into one expandable strip per consecutive run; answers, plans, diffs and
   errors stay. Per-chat override via the header pill; default in Settings. */
const MACHINERY = new Set(["thought", "skill_applied", "model_routed", "terminal"]);
function renderEvents(session: Session, clean: boolean, onRestore?: (text: string) => void, onRewind?: (id: string, label: string) => void) {
  const out: React.ReactNode[] = [];
  const events = session.events;
  // "Edit & retry" on the trailing error: put the failed prompt back in the composer.
  const retryFor = (i: number) => {
    if (!onRestore || i !== events.length - 1) return undefined;
    const lastUser = [...events.slice(0, i)].reverse().find((e) => e.type === "user") as { text?: string } | undefined;
    if (!lastUser?.text) return undefined;
    return () => onRestore(lastUser.text!);
  };
  const item = (ev: Ev, i: number) => (
    <div key={i} className="lectern-msg" style={{ display: "flex", flexDirection: "column", alignItems: ev.type === "user" ? "flex-end" : "stretch" }}>
      <EventView ev={ev} live={session.busy && i === events.length - 1 && ev.type === "message"} onRetry={ev.type === "error" ? retryFor(i) : undefined} onRewind={ev.type === "checkpoint" ? onRewind : undefined} />
    </div>
  );
  if (!clean) {
    events.forEach((ev, i) => { if (ev.type !== "thinking") out.push(item(ev, i)); });
    return out;
  }
  let group: number[] = [];
  const flush = () => {
    if (!group.length) return;
    const idxs = group;
    // live: this strip is the tail of an active run — surface what's happening now.
    const live = session.busy && idxs[idxs.length - 1] === events.length - 1;
    out.push(<MachineryStrip key={`m-${idxs[0]}`} events={events} idxs={idxs} live={live} />);
    group = [];
  };
  events.forEach((ev, i) => {
    if (ev.type === "thinking") return;
    if (MACHINERY.has(ev.type)) { group.push(i); return; }
    flush();
    out.push(item(ev, i));
  });
  flush();
  return out;
}

function MachineryStrip({ events, idxs, live }: { events: Ev[]; idxs: number[]; live?: boolean }) {
  const [open, setOpen] = useState(false);
  const n = idxs.length;
  // Compact summary: say WHAT happened, not just how many rows collapsed.
  let cmds = 0, thoughts = 0, routes = 0, skills = 0;
  for (const i of idxs) {
    const t = events[i].type;
    if (t === "terminal") cmds++;
    else if (t === "thought") thoughts++;
    else if (t === "model_routed") routes++;
    else if (t === "skill_applied") skills++;
  }
  const parts: string[] = [];
  if (cmds) parts.push(`${cmds} command${cmds === 1 ? "" : "s"}`);
  if (thoughts) parts.push(`${thoughts} thought${thoughts === 1 ? "" : "s"}`);
  if (routes) parts.push(`${routes} route${routes === 1 ? "" : "s"}`);
  if (skills) parts.push(`${skills} skill${skills === 1 ? "" : "s"}`);
  const label = parts.length ? parts.join(" · ") : `${n} background step${n === 1 ? "" : "s"}`;
  // Live tail: the most recent command still runs — show it on the strip.
  let tail: string | null = null;
  if (live) {
    for (let k = idxs.length - 1; k >= 0; k--) {
      const e = events[idxs[k]] as { type: string; command?: string; summary?: string };
      if (e.type === "terminal" && e.command) { tail = `$ ${e.command}`; break; }
      if (e.type === "thought" && e.summary) { tail = e.summary; break; }
    }
  }
  return (
    <div className="lectern-msg" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <button onClick={() => setOpen((o) => !o)}
        style={{ alignSelf: "flex-start", maxWidth: "100%", display: "inline-flex", alignItems: "center", gap: 7, border: "1px solid var(--bd)", borderRadius: 8, background: "var(--panel)", color: "var(--fg3)", fontSize: 11.5, fontWeight: 600, padding: "4px 10px", cursor: "pointer", fontFamily: "inherit" }}>
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ transform: open ? "rotate(90deg)" : "none", transition: "transform .18s ease", flexShrink: 0 }} aria-hidden><path d="m9 6 6 6-6 6" /></svg>
        {label}
        {tail && (
          <span className="mono" style={{ fontWeight: 500, color: "var(--fg2)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 320 }}>
            · {tail.length > 60 ? tail.slice(0, 60) + "…" : tail} <span style={{ color: "var(--fg3)" }}>· running</span>
          </span>
        )}
      </button>
      {open && idxs.map((i) => (
        <div key={i} className="lectern-fadein" style={{ display: "flex", flexDirection: "column" }}><EventView ev={events[i]} /></div>
      ))}
    </div>
  );
}

function Chat({ session, backends, models, claudeAvailable, navCollapsed, onShowNav, skillsVersion, personalAgent, dark, cleanDefault, tiled, onPatch, onSend, onCancel, onCommand, onSplit }: {
  session: Session; backends: BackendInfo[]; models: ModelOpt[]; claudeAvailable: boolean; navCollapsed: boolean; onShowNav: () => void; skillsVersion: number; personalAgent?: boolean; dark: boolean; cleanDefault: boolean; tiled?: boolean; onSplit?: (dir: "row" | "col") => void; onPatch: (fn: (s: Session) => Session) => void; onSend: () => void; onCancel: () => void; onCommand: (id: string) => void;
}) {
  const agent = !!personalAgent;
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true); // false once the user scrolls up during a run
  const [unstuck, setUnstuck] = useState(false); // mirrors !pinned for the jump-to-latest button
  const [tree, setTree] = useState<FileEntry[]>(() => treeCache.get(session.path) ?? []);
  const [treeLoading, setTreeLoading] = useState(false);
  useEffect(() => { pinned.current = true; setUnstuck(false); scroller.current?.scrollTo({ top: scroller.current.scrollHeight }); }, [session.events.length, session.busy]);
  // smooth follow while streaming — track rendered growth every frame, but
  // never fight the user: a scroll-up unpins until they return to the bottom
  useEffect(() => {
    if (!session.busy) return;
    let raf = 0;
    const loop = () => {
      const el = scroller.current;
      if (el && pinned.current) el.scrollTop = el.scrollHeight;
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [session.busy]);
  useEffect(() => {
    if (!session.busy) return;
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") onCancel(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [session.busy, onCancel]);
  useEffect(() => {
    if (!session.path.trim()) { setTree([]); setTreeLoading(false); return; }
    const cached = treeCache.get(session.path);
    if (cached) setTree(cached); else { setTree([]); setTreeLoading(true); }
    const t = setTimeout(() => {
      invoke<FileEntry[]>("list_dir", { path: session.path }).then((r) => { treeCache.set(session.path, r); setTree(r); setTreeLoading(false); }).catch(() => setTreeLoading(false));
    }, cached ? 600 : 120);
    return () => clearTimeout(t);
    // session.busy: re-list when a run starts/ends so files the agent wrote appear.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session.path, session.busy]);

  // Rewind the workspace to a checkpoint, then refresh the file tree and drop the original
  // prompt back into the composer so the user can adjust and try again (re-steer).
  const rewindTo = async (id: string, label: string) => {
    try {
      const r = await invoke<{ target: string; redo: string | null; changed: string[] }>("rewind_checkpoint", { path: session.path, id });
      treeCache.delete(session.path);
      invoke<FileEntry[]>("list_dir", { path: session.path }).then((files) => { treeCache.set(session.path, files); setTree(files); }).catch(() => {});
      const n = r.changed.length;
      onPatch((s) => ({
        ...s,
        draft: s.draft.trim() ? s.draft : label,
        events: [...s.events, { type: "thought", summary: n ? `Rewound to ${id} — ${n} file${n === 1 ? "" : "s"} restored` : `Already at ${id}`, recalls: [] }],
      }));
    } catch (e) {
      onPatch((s) => ({ ...s, events: [...s.events, { type: "error", message: `Rewind failed: ${String(e)}` }] }));
    }
  };

  const isClaude = session.backend === "claude-code" || (session.backend === "auto" && claudeAvailable);
  const empty = session.events.length === 0;
  const changes = session.events.filter((e) => e.type === "file_edit") as Extract<Ev, { type: "file_edit" }>[];
  // Suggested Conventional Commit for the run's changes (same engine heuristic as the CLI).
  const [commitMsg, setCommitMsg] = useState("");
  const [commitCopied, setCommitCopied] = useState(false);
  useEffect(() => {
    if (session.busy || changes.length === 0) { setCommitMsg(""); return; }
    invoke<string>("suggest_commit", { changes: changes.map((c) => ({ path: c.path, added: c.added, removed: c.removed })) })
      .then(setCommitMsg).catch(() => setCommitMsg(""));
  }, [session.busy, changes.length]);
  const [termOpen, setTermOpen] = useState(false);
  const [termEver, setTermEver] = useState(false);
  const clean = (session.view ?? (cleanDefault ? "clean" : "verbose")) === "clean";
  /* Mission D6 — context meter: rough estimate (chars/4) of what a fresh turn
     re-sends vs the model's window. Warns before the window drowns; the fix is
     honest: start a fresh chat, the brain carries context over. */
  /* Cheap context estimate (perf: this runs on every streamed chunk — the old
     JSON.stringify over the full history was the main lag-spike source). */
  const ctxTokens = useMemo(() => {
    let chars = 0;
    for (const e of session.events) {
      chars += 40; // structural overhead per event
      const t = (e as { text?: string }).text; if (t) chars += t.length;
      const o = (e as { output?: string }).output; if (o) chars += o.length;
      const sm = (e as { summary?: string }).summary; if (sm) chars += sm.length;
    }
    return Math.round(chars / 4);
  }, [session.events]);
  const ctxWindow = session.backend === "antigravity" ? 1_000_000 : 200_000;
  const ctxPct = Math.min(100, Math.round((ctxTokens / ctxWindow) * 100));
  const terminals = session.events.filter((e) => e.type === "terminal") as Extract<Ev, { type: "terminal" }>[];
  // Agents = the distinct models routed this session (the Conductor's sub-agents show here).
  // Group the transcript into sub-agent segments — each model_routed step plus the events it
  // produced — so the Agents tab can list them and you can "step into" one.
  const agentSegments: { model: string; reason: string; events: Ev[] }[] = [];
  for (const ev of session.events) {
    if (ev.type === "model_routed") agentSegments.push({ model: String((ev as { model: string }).model), reason: String((ev as { reason?: string }).reason ?? ""), events: [] });
    else if (agentSegments.length) agentSegments[agentSegments.length - 1].events.push(ev);
  }
  // Todos = the most recent plan's steps.
  const lastPlan = [...session.events].reverse().find((e) => e.type === "plan") as Extract<Ev, { type: "plan" }> | undefined;
  const todos = lastPlan?.steps ?? [];
  // Workspace · project chips — shown centered under the input on a new session, and as the
  // bottom context bar once a conversation is active. (Model + run mode live in the composer.)
  const contextBar = !personalAgent ? (
    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
      <button onClick={async () => { const p = await invoke<string | null>("pick_folder"); if (p) onPatch((s) => ({ ...s, path: p })); }}
        className="icon-btn" title={session.path || "Choose a project folder"}
        style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 28, maxWidth: 280, padding: "0 11px", borderRadius: 8, border: "1px solid var(--bd)", background: "var(--panel)", color: "var(--fg2)", fontSize: 12.5, cursor: "pointer", fontFamily: "inherit" }}>
        <Icon name="folder" size={14} /> <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{session.path ? baseName(session.path) : "Choose folder"}</span>
      </button>
      {session.project && (
        <span style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 28, padding: "0 11px", borderRadius: 8, border: "1px solid var(--bd)", background: "var(--panel)", color: "var(--fg2)", fontSize: 12.5 }}>
          <Icon name="folder" size={13} /> {session.project}
        </span>
      )}
    </div>
  ) : null;
  const [rightTab, setRightTab] = useState<"files" | "agents" | "shells" | "todos" | "preview">("files");
  // Preview rail v1 (design: docs/preview-rail-design.md — lives as a work-panel
  // tab: ONE right rail, no dueling layouts). Items derive from events; the
  // selection is a pin — new events never steal the view.
  const previewItems = useMemo(() => {
    const files = new Map<string, { added: number; removed: number }>();
    const urls = new Set<string>();
    for (const ev of session.events) {
      if (ev.type === "file_edit") {
        const e = ev as { path?: string; added?: number; removed?: number };
        if (e.path) files.set(e.path, { added: e.added ?? 0, removed: e.removed ?? 0 });
      } else if (ev.type === "message" || ev.type === "user") {
        const t = String((ev as { text?: string }).text ?? "");
        for (const m of t.matchAll(/https?:\/\/[^\s)"'`>\]]+/g)) urls.add(m[0].replace(/[.,;:]+$/, ""));
      }
    }
    const items = [
      ...[...files.entries()].map(([path, d]) => ({ kind: "file" as const, id: path, label: path.split("/").pop() ?? path, detail: `+${d.added} −${d.removed}` })),
    ];
    // Workspace artifacts: renderable files at the workspace root, regardless of
    // which backend wrote them — backends that edit in place (e.g. opencode) never
    // emit file_edit events, so the event-derived list alone misses their output.
    const eventPaths = [...files.keys()];
    for (const f of tree) {
      if (!f.dir && /\.(html?|svg|md|png|jpe?g|gif|webp)$/i.test(f.name)
        && !eventPaths.some((p) => p === f.name || p.endsWith(`/${f.name}`))) {
        items.push({ kind: "file" as const, id: f.name, label: f.name, detail: "" });
      }
    }
    return [
      ...items,
      ...[...urls].slice(0, 10).map((u) => ({ kind: "url" as const, id: u, label: u.replace(/^https?:\/\//, "").slice(0, 34), detail: "" })),
    ];
  }, [session.events, tree]);
  const [prevSel, setPrevSel] = useState<string | null>(null);
  const [prevText, setPrevText] = useState<string>("");
  const [prevSrcView, setPrevSrcView] = useState(false); // html artifacts: rendered ↔ source
  // Artifact version history: each time the rail refetches a file and its content
  // changed, snapshot it (session-lifetime, capped). The stepper flips between them.
  const prevVersions = useRef<Map<string, { ts: number; text: string }[]>>(new Map());
  const [, bumpVers] = useState(0);
  const [prevVer, setPrevVer] = useState<number | null>(null); // null = live (latest)
  const prevItem = previewItems.find((it) => it.id === prevSel) ?? null;
  const prevAbs = prevItem && prevItem.kind === "file"
    ? (prevItem.id.startsWith("/") ? prevItem.id : `${session.path}/${prevItem.id}`)
    : null;
  const prevVers = prevAbs ? (prevVersions.current.get(prevAbs) ?? []) : [];
  const prevShownText = prevVer == null ? prevText : (prevVers[prevVer]?.text ?? prevText);
  useEffect(() => { setPrevVer(null); }, [prevSel]);
  useEffect(() => {
    if (!prevItem || prevItem.kind !== "file") { setPrevText(""); return; }
    const abs = prevItem.id.startsWith("/") ? prevItem.id : `${session.path}/${prevItem.id}`;
    if (/\.(png|jpe?g|gif|svg|webp)$/i.test(abs)) { setPrevText(""); return; }
    invoke<string>("read_text_file", { path: abs }).then((t) => {
      setPrevText(t);
      const vs = prevVersions.current.get(abs) ?? [];
      if (!vs.length || vs[vs.length - 1].text !== t) {
        vs.push({ ts: Date.now(), text: t });
        if (vs.length > 20) vs.shift();
        prevVersions.current.set(abs, vs);
        bumpVers((n) => n + 1);
      }
    }).catch((e) => setPrevText(`⚠ ${String(e)}`));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [prevSel, session.path, session.events.length]);
  const [showPanel, setShowPanel] = useState(true); // Files/Agents/Shells/Todos panel
  const [openFile, setOpenFile] = useState<{ path: string; name: string } | null>(null); // built-in editor
  const [stepIn, setStepIn] = useState<number | null>(null); // sub-agent step-in
  const [shared, setShared] = useState(false); // Share button "Copied ✓" flash (index into agentSegments)
  const hasRepo = !!session.path.trim();
  const editFile = (p: string, n: string) => setOpenFile({ path: p, name: n });

  return (
    <div style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column", position: "relative" }}>
      {/* top chrome bar — sidebar/panel toggles + actions live here (clean "focus mode"
          controls), instead of floating buttons. */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, height: 46, flexShrink: 0, padding: "0 12px", borderBottom: "1px solid var(--bd)", background: "var(--panel)" }}>
        {navCollapsed && (
          <button onClick={onShowNav} className="icon-btn" title="Show sidebar" style={{ width: 32, height: 32, borderRadius: 8, border: "none", background: "transparent", color: "var(--fg2)", cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center" }}><Icon name="panelLeft" size={17} /></button>
        )}
        {agent && (
          <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
            <span style={{ color: ACCENT, display: "flex" }}><Icon name="agent" size={16} /></span>
            <span style={{ fontWeight: 600, fontSize: 13.5 }}>Personal Agent</span>
          </div>
        )}
        <div style={{ flex: 1 }} />
        {onSplit && !tiled && (
          <button title="Tile: split this chat side-by-side (tmux-style)" onClick={() => onSplit("row")}
            style={{ height: 28, width: 30, display: "inline-flex", alignItems: "center", justifyContent: "center", borderRadius: 8, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg3)", cursor: "pointer", flexShrink: 0 }}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden><rect x="3" y="4" width="18" height="16" rx="2.5" /><path d="M12 4v16" /></svg>
          </button>
        )}
        <button title={termOpen ? "Hide the terminal" : "Open a terminal in this chat's folder"}
          onClick={() => { setTermOpen((v) => !v); setTermEver(true); }}
          style={{ height: 28, width: 30, display: "inline-flex", alignItems: "center", justifyContent: "center", borderRadius: 8, border: "1px solid var(--bd)", background: termOpen ? "var(--hov)" : "transparent", color: termOpen ? "var(--fg)" : "var(--fg3)", cursor: "pointer", flexShrink: 0 }}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden><rect x="3" y="4.5" width="18" height="15" rx="2.5" /><path d="m7.5 9.5 3 3-3 3M12.5 15.5h4.5" /></svg>
        </button>
        {ctxTokens > 1000 && (
          <span className="mono" title={`≈${ctxTokens.toLocaleString()} tokens of chat context vs a ~${(ctxWindow / 1000).toLocaleString()}k window. ${ctxPct >= 70 ? "Getting full — start a fresh chat; Lectern's brain carries the important context over." : "Plenty of room."}`}
            style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px", borderRadius: 8, border: `1px solid ${ctxPct >= 90 ? DANGER : ctxPct >= 70 ? WARN : "var(--bd)"}`, color: ctxPct >= 90 ? DANGER : ctxPct >= 70 ? WARN : "var(--fg3)", fontSize: 10.5, flexShrink: 0 }}>
            <span style={{ width: 30, height: 5, borderRadius: 999, background: "var(--panel2)", overflow: "hidden", display: "inline-block" }}>
              <span style={{ display: "block", width: `${Math.max(4, ctxPct)}%`, height: "100%", background: "currentColor", borderRadius: 999 }} />
            </span>
            {ctxPct}%{ctxPct >= 70 ? " · fresh chat?" : ""}
          </span>
        )}
        <button title={clean ? "Showing the clean view — click for every step" : "Showing every step — click for the clean view"}
          onClick={() => onPatch((s) => ({ ...s, view: clean ? "verbose" : "clean" }))}
          style={{ height: 28, padding: "0 11px", borderRadius: 8, border: "1px solid var(--bd)", background: clean ? "var(--hov)" : "transparent", color: clean ? "var(--fg)" : "var(--fg3)", fontSize: 11.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>
          {clean ? "Clean" : "Verbose"}
        </button>
        <button onClick={() => { const text = session.events.map((e) => e.type === "user" ? `You: ${(e as { text?: string }).text ?? ""}` : (e as { text?: string }).text ?? "").filter(Boolean).join("\n\n"); navigator.clipboard?.writeText(text); setShared(true); setTimeout(() => setShared(false), 1600); }}
          className="icon-btn" title="Copy this conversation" disabled={empty}
          style={{ height: 30, padding: "0 12px", borderRadius: 8, border: "1px solid var(--bd)", background: shared ? "var(--hov)" : "transparent", color: shared ? "var(--fg)" : "var(--fg2)", fontSize: 12.5, fontWeight: 600, cursor: empty ? "default" : "pointer", fontFamily: "inherit", display: "inline-flex", alignItems: "center", gap: 6, opacity: empty ? 0.45 : 1 }}>{shared ? "Copied ✓" : "Share"}</button>
        {(hasRepo || agent) && !empty && (
          <button onClick={() => setShowPanel((v) => !v)} className="icon-btn" title={showPanel ? "Hide panel" : "Show panel"} style={{ width: 32, height: 32, borderRadius: 8, border: "none", background: "transparent", color: "var(--fg2)", cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center" }}><Icon name={showPanel ? "panelRightClose" : "panelRight"} size={17} /></button>
        )}
      </div>

      {/* 3-pane body */}
      <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
        {/* left pane: MCP servers for the agent (the file tree now lives in the right panel's Files tab) */}

        {/* conversation */}
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", position: "relative" }}>
          <div
            ref={scroller}
            onScroll={(e) => { const el = e.currentTarget; const stick = el.scrollHeight - el.scrollTop - el.clientHeight < 56; pinned.current = stick; setUnstuck(!stick); }}
            style={{ flex: 1, overflow: "auto" }}
          >
            {empty ? (
              <div style={{ minHeight: "100%", display: "flex", alignItems: "center", justifyContent: "center", padding: "40px 24px" }}>
                <div className="lectern-msg" style={{ width: "100%", maxWidth: COL, display: "flex", flexDirection: "column", gap: 16 }}>
                  <div style={{ textAlign: "center" }}>
                    <div style={{ display: "flex", justifyContent: "center", marginBottom: 16, opacity: 0.92 }}><div style={{ transform: "scale(1.5)" }}><Logo /></div></div>
                    <div style={{ fontSize: 30, fontWeight: 800, letterSpacing: "-0.025em" }}>{agent ? "Your personal desktop agent" : "What should we build?"}</div>
                    <div style={{ fontSize: 14, lineHeight: 1.55, color: "var(--fg2)", maxWidth: 460, margin: "10px auto 0" }}>
                      {agent ? "I can see and control your screen — open apps, click, type, and run tasks across your whole computer — using the same memory and skills as your projects." : !hasRepo ? "Open a project folder to begin." : isClaude ? "Claude Code, with Lectern's memory and learned skills for this repo." : "Lectern remembers this repo and applies your learned skills."}
                    </div>
                  </div>
                  {!agent && !hasRepo ? (
                    <div style={{ textAlign: "center" }}>
                      <button onClick={async () => { const p = await invoke<string | null>("pick_folder"); if (p) onPatch((s) => ({ ...s, path: p })); }}
                        style={{ height: 38, padding: "0 18px", borderRadius: 9, border: "none", background: "var(--btn)", color: "var(--btnfg)", fontSize: 14, fontWeight: 600, cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center", gap: 8, fontFamily: "inherit" }}><Icon name="folder" size={16} /> Open a project</button>
                    </div>
                  ) : (
                    <>
                      <Composer session={session} isClaude={isClaude} personalAgent={agent} skillsVersion={skillsVersion} backends={backends} models={models} onPatch={onPatch} onSend={onSend} onCancel={onCancel} onCommand={onCommand} />
                      {contextBar && <div style={{ padding: "0 20px" }}><div style={{ maxWidth: COL, margin: "0 auto" }}>{contextBar}</div></div>}
                      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, justifyContent: "center", padding: "4px 20px 0" }}>
                        {(agent ? ["Take a screenshot of my screen", "Open Firefox and search the weather", "Tidy up my Downloads folder"] : ["Add a dark-mode toggle", "Find and fix the failing test", "Explain how this codebase works"]).map((sug) => (
                          <button key={sug} onClick={() => onPatch((s) => ({ ...s, draft: sug }))} style={{ border: "1px solid var(--bd)", borderRadius: 999, padding: "6px 13px", fontSize: 12.5, cursor: "pointer", background: "var(--panel)", color: "var(--fg2)", fontFamily: "inherit" }}>{sug}</button>
                        ))}
                      </div>
                    </>
                  )}
                </div>
              </div>
            ) : (
              <div style={{ maxWidth: COL, margin: "0 auto", padding: "26px 24px 30px", display: "flex", flexDirection: "column", gap: 15 }}>
                {renderEvents(session, clean, (text) => onPatch((s) => ({ ...s, draft: text })), rewindTo)}
                {session.busy && <Working tokens={runningTokens(session.events)} />}
                {session.summary && <SummaryView s={session.summary} />}
              </div>
            )}
          </div>
          {termEver && (
            <Suspense fallback={null}>
              <TerminalDrawer sessionId={session.id} cwd={session.path || ""} visible={termOpen} onExit={() => setTermOpen(false)} />
            </Suspense>
          )}
          {/* jump to latest — appears when the user scrolled up while content grows */}
          {!empty && unstuck && (
            <button
              onClick={() => { const el = scroller.current; if (el) el.scrollTo({ top: el.scrollHeight, behavior: "smooth" }); pinned.current = true; setUnstuck(false); }}
              title="Jump to latest"
              className="icon-btn"
              style={{ position: "absolute", left: "50%", transform: "translateX(-50%)", bottom: 168, zIndex: 5, width: 34, height: 34, borderRadius: 999, border: "1px solid var(--bd)", background: "var(--panel)", color: "var(--fg2)", cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center", boxShadow: "var(--shadow-pop)" }}>
              <Icon name="chevron" size={16} />
            </button>
          )}
          {!empty && <Composer session={session} isClaude={isClaude} personalAgent={agent} skillsVersion={skillsVersion} backends={backends} models={models} onPatch={onPatch} onSend={onSend} onCancel={onCancel} onCommand={onCommand} />}
          {!empty && contextBar && (
            <div style={{ flexShrink: 0, padding: "0 20px 14px" }}>
              <div style={{ maxWidth: COL, margin: "0 auto" }}>{contextBar}</div>
            </div>
          )}
        </div>

        {/* right panel: Files · Agents · Shells · Todos (Omnigent-style work panel) */}
        {showPanel && (hasRepo || agent) && !empty && (
          <div className="lectern-side" style={{ width: 256, flexShrink: 0, borderLeft: "1px solid var(--bd)", background: "var(--panel)", overflow: "auto", padding: "14px 13px", display: "flex", flexDirection: "column", gap: 12 }}>
            <div style={{ display: "flex", gap: 2 }}>
              {([["files", "Files", tree.length], ["agents", "Agents", agentSegments.length], ["shells", "Shells", terminals.length], ["preview", "Preview", previewItems.length], ["todos", "Todos", todos.length]] as const).map(([id, label, n]) => (
                <button key={id} onClick={() => setRightTab(id)} style={{ flex: 1, height: 30, borderRadius: 8, border: "none", background: rightTab === id ? "var(--hov)" : "transparent", color: rightTab === id ? "var(--fg)" : "var(--fg2)", fontSize: 11.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit", display: "flex", alignItems: "center", justifyContent: "center", gap: 4 }}>{label}{n > 0 ? <span className="mono" style={{ fontSize: 9.5, color: "var(--fg3)" }}>{n}</span> : null}</button>
              ))}
            </div>

            {rightTab === "files" && (
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                <div style={{ fontSize: 11, color: "var(--fg3)" }}>Working folder · <span style={{ color: "var(--fg2)" }}>{session.path ? baseName(session.path) : "—"}</span></div>
                {changes.length > 0 && (
                  <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                    <div style={{ fontSize: 11, fontWeight: 600, color: "var(--fg3)" }}>Changed</div>
                    {changes.map((c, i) => (
                      <div key={i} onClick={() => editFile(c.path, baseName(c.path))} title={`Open ${baseName(c.path)} in the editor`} className="mono lectern-row" style={{ display: "flex", justifyContent: "space-between", gap: 8, fontSize: 11.5, color: "var(--fg2)", cursor: "pointer", borderRadius: 5, padding: "1px 3px" }}>
                        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{baseName(c.path)}</span>
                        <span style={{ flexShrink: 0, opacity: 0.85 }}><span style={{ color: ACCENT }}>+{c.added}</span> <span style={{ color: DANGER }}>−{c.removed}</span></span>
                      </div>
                    ))}
                    {commitMsg && (
                      <div style={{ marginTop: 4, border: "1px solid var(--bd2)", borderRadius: 7, padding: "6px 8px", display: "flex", alignItems: "center", gap: 8, background: "var(--panel2)" }}>
                        <span className="mono" title={commitMsg} style={{ flex: 1, minWidth: 0, fontSize: 11, color: "var(--fg2)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{commitMsg}</span>
                        <button title="Copy suggested commit message" onClick={() => { navigator.clipboard?.writeText(commitMsg).then(() => { setCommitCopied(true); setTimeout(() => setCommitCopied(false), 1400); }).catch(() => {}); }}
                          style={{ flexShrink: 0, border: "none", background: "transparent", color: commitCopied ? ACCENT : "var(--fg3)", cursor: "pointer", fontSize: 10.5, fontWeight: 600, fontFamily: "inherit", padding: 0 }}>
                          {commitCopied ? "copied ✓" : "copy commit"}
                        </button>
                      </div>
                    )}
                  </div>
                )}
                <div className="mono" style={{ fontSize: 12, color: "var(--fg2)", display: "flex", flexDirection: "column", gap: 4 }}>
                  {treeLoading && tree.length === 0
                    ? [72, 54, 84, 48, 66].map((w, i) => <div key={i} className="lectern-skel" style={{ height: 11, width: `${w}%` }} />)
                    : tree.length === 0 ? <span style={{ color: "var(--fg3)" }}>Empty folder.</span> : tree.map((f) => (
                      <span key={f.name} onClick={() => !f.dir && editFile(`${session.path}/${f.name}`, f.name)}
                        title={f.dir ? f.name : `Open ${f.name} in the editor`}
                        style={{ display: "inline-flex", alignItems: "center", gap: 6, cursor: f.dir ? "default" : "pointer", color: f.dir ? "var(--fg2)" : "var(--fg)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                        {f.dir ? <Icon name="folder" size={13} /> : <span style={{ width: 13 }} />}{f.name}
                      </span>
                    ))}
                </div>
              </div>
            )}

            {rightTab === "agents" && (
              agentSegments.length === 0 ? (
                <div style={{ fontSize: 12, color: "var(--fg3)", lineHeight: 1.6 }}>No sub-agents yet. When the Conductor routes a task, each model it uses shows here — click one to step into its work.</div>
              ) : (
                <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                  {agentSegments.map((seg, i) => (
                    <button key={i} onClick={() => setStepIn(i)} title="Step into this sub-agent's work"
                      style={{ display: "flex", alignItems: "center", gap: 8, border: "1px solid var(--bd)", borderRadius: 9, background: "var(--bg)", padding: "8px 10px", fontSize: 12.5, cursor: "pointer", fontFamily: "inherit", textAlign: "left", color: "var(--fg)", width: "100%" }}>
                      <span style={{ color: ACCENT, display: "flex", flexShrink: 0 }}><Icon name="agent" size={14} /></span>
                      <span style={{ flex: 1, minWidth: 0 }}>
                        <span style={{ display: "block", fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{i + 1}. {seg.model}</span>
                        <span style={{ display: "block", fontSize: 11, color: "var(--fg3)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{seg.reason || `${seg.events.length} event${seg.events.length === 1 ? "" : "s"}`}</span>
                      </span>
                      <span className="mono" style={{ fontSize: 9.5, color: "var(--fg3)", flexShrink: 0 }}>{seg.events.length}</span>
                    </button>
                  ))}
                </div>
              )
            )}

            {rightTab === "shells" && (
              terminals.length === 0 ? (
                <div style={{ fontSize: 12, color: "var(--fg3)", lineHeight: 1.6 }}>No commands yet. Every command the agent runs shows here.</div>
              ) : (
                <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                  {terminals.map((t, i) => (
                    <div key={i} className="mono" style={{ border: `1px solid ${t.exit_code !== 0 ? "#e5a0a0" : "var(--bd)"}`, borderRadius: 9, background: "var(--bg)", fontSize: 10.5, lineHeight: 1.5, overflow: "hidden" }}>
                      <div style={{ padding: "6px 9px", color: "var(--fg2)", borderBottom: t.output ? "1px solid var(--bd2)" : "none", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>$ {t.command}</div>
                      {t.output && <div style={{ padding: "6px 9px", color: t.exit_code !== 0 ? DANGER : "var(--fg2)", whiteSpace: "pre-wrap", maxHeight: 150, overflow: "auto" }}>{t.output}</div>}
                    </div>
                  ))}
                </div>
              )
            )}

            {rightTab === "todos" && (
              todos.length === 0 ? (
                <div style={{ fontSize: 12, color: "var(--fg3)", lineHeight: 1.6 }}>No plan yet. The Conductor's steps show here as a checklist.</div>
              ) : (
                <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
                  {todos.map((st, i) => (
                    <div key={i} style={{ display: "flex", gap: 8, fontSize: 12.5, lineHeight: 1.5, color: st.done ? "var(--fg2)" : "var(--fg)" }}>
                      <span style={{ color: st.done ? ACCENT : "var(--fg3)", flexShrink: 0 }}>{st.done ? "✓" : "○"}</span>
                      <span style={{ textDecoration: st.done ? "line-through" : "none" }}>{st.text}</span>
                    </div>
                  ))}
                </div>
              )
            )}
            {rightTab === "preview" && (
              previewItems.length === 0 ? (
                <div style={{ fontSize: 12, color: "var(--fg3)", lineHeight: 1.5 }}>Nothing to preview yet — edited files and mentioned links show up here as the agent works.</div>
              ) : (
                <div style={{ display: "flex", flexDirection: "column", gap: 8, minHeight: 0, flex: 1 }}>
                  <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
                    {previewItems.map((it) => (
                      <button key={it.id} onClick={() => setPrevSel(it.id === prevSel ? null : it.id)}
                        title={it.id}
                        style={{ height: 24, padding: "0 9px", borderRadius: 7, border: "1px solid var(--bd)", background: prevSel === it.id ? "var(--hov)" : "transparent", color: prevSel === it.id ? "var(--fg)" : "var(--fg2)", fontSize: 11, fontWeight: 600, cursor: "pointer", fontFamily: "inherit", display: "inline-flex", alignItems: "center", gap: 5, maxWidth: "100%" }}>
                        {it.kind === "url" && <Globe size={11} strokeWidth={1.8} style={{ flexShrink: 0 }} />}
                        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{it.label}</span>
                        {it.detail && <span className="mono" style={{ fontSize: 9.5, color: "var(--fg3)" }}>{it.detail}</span>}
                      </button>
                    ))}
                  </div>
                  {prevItem ? (
                    prevItem.kind === "url" ? (
                      <div style={{ flex: 1, minHeight: 240, border: "1px solid var(--bd)", borderRadius: 10, overflow: "hidden", display: "flex", flexDirection: "column" }}>
                        <iframe src={prevItem.id} title={prevItem.label} style={{ flex: 1, border: "none", background: "#fff" }} sandbox="allow-scripts allow-same-origin" />
                        <div className="mono" style={{ fontSize: 10, color: "var(--fg3)", padding: "5px 9px", borderTop: "1px solid var(--bd)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{prevItem.id}</div>
                      </div>
                    ) : /\.(png|jpe?g|gif|svg|webp)$/i.test(prevItem.id) ? (
                      <div style={{ border: "1px solid var(--bd)", borderRadius: 10, padding: 8, background: "var(--panel2)" }}>
                        <img src={convertFileSrc(prevItem.id.startsWith("/") ? prevItem.id : `${session.path}/${prevItem.id}`)} alt={prevItem.label} style={{ maxWidth: "100%", borderRadius: 6 }} />
                      </div>
                    ) : /\.html?$/i.test(prevItem.id) ? (
                      /* HTML artifact → live render, sandboxed. No allow-same-origin: the
                         document runs isolated from the app (no tauri APIs, no storage). */
                      <div style={{ flex: 1, minHeight: 240, border: "1px solid var(--bd)", borderRadius: 10, overflow: "hidden", display: "flex", flexDirection: "column" }}>
                        {prevSrcView ? (
                          <pre className="mono" style={{ flex: 1, margin: 0, overflow: "auto", fontSize: 11, lineHeight: 1.55, whiteSpace: "pre-wrap", wordBreak: "break-word", color: "var(--fg2)", padding: "10px 12px", background: "var(--panel2)" }}>{prevShownText}</pre>
                        ) : (
                          <iframe srcDoc={prevShownText} title={prevItem.label} sandbox="allow-scripts" style={{ flex: 1, border: "none", background: "#fff" }} />
                        )}
                        <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "4px 6px 4px 9px", borderTop: "1px solid var(--bd)" }}>
                          <span className="mono" style={{ flex: 1, fontSize: 10, color: "var(--fg3)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                            {prevItem.label} · {prevSrcView ? "source" : "rendered · sandboxed"}
                            {prevVer != null && ` · viewing v${prevVer + 1} of ${prevVers.length}`}
                          </span>
                          {prevVers.length > 1 && (
                            <span style={{ display: "inline-flex", alignItems: "center", gap: 2, flexShrink: 0 }}>
                              <button className="icon-btn" title="Older version"
                                disabled={(prevVer ?? prevVers.length - 1) <= 0}
                                onClick={() => setPrevVer((v) => Math.max(0, (v ?? prevVers.length - 1) - 1))}
                                style={{ width: 22, height: 22, borderRadius: 6, border: "none", background: "transparent", color: "var(--fg2)", cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center", padding: 0, opacity: (prevVer ?? prevVers.length - 1) <= 0 ? 0.35 : 1 }}>
                                <ChevronLeft size={13} strokeWidth={1.8} />
                              </button>
                              <span className="mono" style={{ fontSize: 10, color: "var(--fg3)" }}>
                                v{(prevVer ?? prevVers.length - 1) + 1}/{prevVers.length}
                              </span>
                              <button className="icon-btn" title="Newer version"
                                disabled={prevVer == null}
                                onClick={() => setPrevVer((v) => (v == null || v >= prevVers.length - 2 ? null : v + 1))}
                                style={{ width: 22, height: 22, borderRadius: 6, border: "none", background: "transparent", color: "var(--fg2)", cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center", padding: 0, opacity: prevVer == null ? 0.35 : 1 }}>
                                <ChevronRight size={13} strokeWidth={1.8} />
                              </button>
                            </span>
                          )}
                          <button className="icon-btn" onClick={() => setPrevSrcView((v) => !v)} title={prevSrcView ? "Show rendered" : "Show source"}
                            style={{ width: 24, height: 24, borderRadius: 6, border: "none", background: "transparent", color: "var(--fg2)", cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center", padding: 0 }}>
                            {prevSrcView ? <Globe size={13} strokeWidth={1.8} /> : <Code size={13} strokeWidth={1.8} />}
                          </button>
                        </div>
                      </div>
                    ) : (
                      <div style={{ flex: 1, minHeight: 0, overflow: "auto", border: "1px solid var(--bd)", borderRadius: 10, padding: "10px 12px", background: "var(--panel2)" }}>
                        {prevItem.id.endsWith(".md") ? (
                          <Markdown text={prevText} />
                        ) : (
                          <pre className="mono" style={{ margin: 0, fontSize: 11, lineHeight: 1.55, whiteSpace: "pre-wrap", wordBreak: "break-word", color: "var(--fg2)" }}>{prevText}</pre>
                        )}
                      </div>
                    )
                  ) : (
                    <div style={{ fontSize: 11.5, color: "var(--fg3)" }}>Pick something above to preview it — your pick stays pinned while the agent keeps working.</div>
                  )}
                </div>
              )
            )}
          </div>
        )}
      </div>
      {openFile && (
        <Suspense fallback={null}>
          <CodeEditor
            path={openFile.path}
            name={openFile.name}
            dark={dark}
            onClose={() => setOpenFile(null)}
            onAddToPrompt={(t) => onPatch((s) => ({ ...s, draft: t }))}
          />
        </Suspense>
      )}
      {stepIn != null && agentSegments[stepIn] && (
        <div style={{ position: "absolute", inset: 0, background: "var(--bg)", display: "flex", flexDirection: "column", zIndex: 40 }}>
          <div style={{ height: 46, flexShrink: 0, borderBottom: "1px solid var(--bd)", display: "flex", alignItems: "center", gap: 10, padding: "0 12px" }}>
            <span style={{ color: ACCENT, display: "flex", flexShrink: 0 }}><Icon name="agent" size={16} /></span>
            <span style={{ fontWeight: 600, fontSize: 13.5, flexShrink: 0 }}>Step {stepIn + 1} · {agentSegments[stepIn].model}</span>
            <span style={{ fontSize: 11.5, color: "var(--fg3)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{agentSegments[stepIn].reason}</span>
            <button onClick={() => setStepIn(null)} style={{ marginLeft: "auto", flexShrink: 0, height: 28, padding: "0 12px", borderRadius: 8, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg2)", fontSize: 12.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>Close</button>
          </div>
          <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "22px 24px" }}>
            <div style={{ maxWidth: COL, margin: "0 auto", display: "flex", flexDirection: "column", gap: 15 }}>
              {agentSegments[stepIn].events.length === 0
                ? <div style={{ color: "var(--fg3)", fontSize: 13 }}>This sub-agent hasn't produced output yet.</div>
                : agentSegments[stepIn].events.map((ev, i) => ev.type === "thinking" ? null : (
                  <div key={i} className="lectern-msg" style={{ display: "flex", flexDirection: "column", alignItems: ev.type === "user" ? "flex-end" : "stretch" }}><EventView ev={ev} /></div>
                ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// The model menu: concrete models grouped by provider. Picking one sets BOTH the harness
// (backend) and the model — "Auto" lets Lectern route per task. A model is selectable only
// when its provider's CLI is connected (Claude Code / Antigravity), so it's obvious which
// models you can actually use right now.
export type ModelOpt = { id: string; label: string; backend: string; model: string; group: string };
/* Static fallback only — the live list comes from `claude_models` (models this
   account has actually used, read from ~/.claude.json), so new models like
   Fable 5 / Sonnet 5 appear without an app update. */
const STATIC_CLAUDE: ModelOpt[] = [
  { id: "opus", label: "Claude Opus 4.8", backend: "claude-code", model: "opus", group: "Claude · Claude Code" },
  { id: "sonnet", label: "Claude Sonnet 4.6", backend: "claude-code", model: "sonnet", group: "Claude · Claude Code" },
  { id: "haiku", label: "Claude Haiku 4.5", backend: "claude-code", model: "haiku", group: "Claude · Claude Code" },
];
// Local models the 2026 rankings call strong for coding — flagged in the picker so a
// user choosing a local (Ollama) model lands on a capable one instead of a general chat
// model. Matched as substrings of the model id.
const CODING_MODEL_HINTS = ["qwen3-coder", "qwen2.5-coder", "qwen-coder", "devstral", "deepseek-coder", "deepseek-v", "codellama", "codestral", "starcoder", "glm", "gpt-oss"];
const isStrongCoder = (id: string) => CODING_MODEL_HINTS.some((c) => id.toLowerCase().includes(c));

function modelOptions(claude: ModelInfo[], opencode: ModelInfo[] = [], openrouter: ModelInfo[] = [], ollama: ModelInfo[] = []): ModelOpt[] {
  const claudeOpts: ModelOpt[] = claude.length
    ? claude.map((m) => ({ id: m.id, label: `Claude ${m.label}`, backend: "claude-code", model: m.id, group: "Claude · Claude Code" }))
    : STATIC_CLAUDE;
  return [
    { id: "auto", label: "Auto — best model per task", backend: "auto", model: "", group: "" },
    ...claudeOpts,
    { id: "gemini-flash", label: "Gemini 3.5 Flash", backend: "antigravity", model: "Gemini 3.5 Flash (High)", group: "Antigravity · Google AI" },
    { id: "gemini-pro", label: "Gemini 3.1 Pro", backend: "antigravity", model: "Gemini 3.1 Pro (High)", group: "Antigravity · Google AI" },
    { id: "gpt-oss", label: "GPT-OSS 120B", backend: "antigravity", model: "GPT-OSS 120B (Medium)", group: "Antigravity · Google AI" },
    ...(opencode.length
      ? opencode.map((m) => ({ id: m.id, label: m.label, backend: "opencode", model: m.id, group: m.id.endsWith("-free") ? "Free — no key needed" : "OpenCode · OpenRouter & more" }))
      : [
          { id: "opencode/deepseek-v4-flash-free", label: "DeepSeek V4 Flash (free)", backend: "opencode", model: "opencode/deepseek-v4-flash-free", group: "Free — no key needed" },
          { id: "opencode/nemotron-3-ultra-free", label: "Nemotron 3 Ultra (free)", backend: "opencode", model: "opencode/nemotron-3-ultra-free", group: "Free — no key needed" },
        ]),
    ...openrouter.map((m) => ({ id: m.id, label: `OpenRouter ${m.label}`, backend: "openrouter", model: m.id, group: m.id.endsWith(":free") ? "Free — no key needed" : "OpenRouter" })),
    ...ollama.map((m) => ({ id: m.id, label: isStrongCoder(m.id) ? `${m.label} · good for code` : m.label, backend: "ollama", model: m.id, group: "Ollama · local" })),
  ];
}

function highlightDraft(text: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  const re = /(^|\s)(\/[a-zA-Z][\w-]*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let i = 0;
  while ((m = re.exec(text)) !== null) {
    const start = m.index + m[1].length;
    if (start > last) nodes.push(text.slice(last, start));
    nodes.push(<span key={i++} style={{ background: "rgba(127,127,127,.22)", borderRadius: 5, color: "var(--fg)" }}>{m[2]}</span>);
    last = start + m[2].length;
  }
  nodes.push(text.slice(last));
  return nodes;
}

// In-chat scheduling: queue the current prompt to run later, in this session's folder with
// its current model + run mode. The Schedule page is the read-only overview of everything.
function ScheduleControl({ session, onScheduled }: { session: Session; onScheduled: (text: string) => void }) {
  const [open, setOpen] = useState(false);
  const [when, setWhen] = useState("");
  const ready = !!session.draft.trim() && !!when && !!session.path.trim();
  const schedule = async () => {
    if (!ready) return;
    const runAt = Math.floor(new Date(when).getTime() / 1000);
    try { await invoke("schedule_add", { path: session.path, prompt: session.draft.trim(), backend: session.backend, apply: session.apply, runAt }); } catch { /* surfaced on the Schedule page */ }
    onScheduled(`Scheduled to run “${truncate(session.draft.trim(), 50)}” at ${new Date(runAt * 1000).toLocaleString()}.`);
    setOpen(false); setWhen("");
  };
  const label = !session.path.trim() ? "Open a folder first" : !session.draft.trim() ? "Type a prompt first" : !when ? "Pick a time" : "Schedule";
  return (
    <div style={{ position: "relative", flexShrink: 0 }}>
      <button onClick={() => setOpen((o) => !o)} className="icon-btn" title="Schedule this prompt for later"
        style={{ width: 32, height: 32, borderRadius: 9, border: "none", background: "transparent", color: "var(--fg3)", cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center", padding: 0 }}>
        <Icon name="schedule" size={18} />
      </button>
      {open && (
        <>
          <div onClick={() => setOpen(false)} style={{ position: "fixed", inset: 0, zIndex: 25 }} />
          <div style={{ position: "absolute", bottom: "calc(100% + 10px)", right: 0, width: 300, background: "var(--panel)", border: "1px solid var(--bd)", borderRadius: 12, boxShadow: "0 18px 50px -16px rgba(0,0,0,.45)", padding: 13, zIndex: 26, display: "flex", flexDirection: "column", gap: 9 }}>
            <div style={{ fontSize: 13, fontWeight: 600 }}>Schedule this prompt</div>
            <div style={{ fontSize: 11.5, color: "var(--fg3)", lineHeight: 1.45 }}>Runs later in {session.path ? baseName(session.path) : "this folder"}, with the model + run mode you've set. The Lectern daemon runs it when due.</div>
            <input type="datetime-local" value={when} onChange={(e) => setWhen(e.target.value)}
              style={{ height: 36, boxSizing: "border-box", borderRadius: 8, border: "1px solid var(--bd)", background: "var(--bg)", color: "var(--fg)", fontSize: 13, padding: "0 11px", outline: "none", fontFamily: "inherit", width: "100%" }} />
            <button onClick={ready ? schedule : undefined} disabled={!ready}
              style={{ height: 34, borderRadius: 8, border: "none", background: ready ? "var(--btn)" : "var(--bd)", color: ready ? "var(--btnfg)" : "var(--fg3)", fontSize: 13, fontWeight: 700, cursor: ready ? "pointer" : "default", fontFamily: "inherit" }}>{label}</button>
          </div>
        </>
      )}
    </div>
  );
}

function Composer({ session, isClaude, personalAgent, skillsVersion, backends, models, onPatch, onSend, onCancel, onCommand }: { session: Session; isClaude: boolean; personalAgent?: boolean; skillsVersion: number; backends: BackendInfo[]; models: ModelOpt[]; onPatch: (fn: (s: Session) => Session) => void; onSend: () => void; onCancel: () => void; onCommand: (id: string) => void }) {
  const noPath = !session.path.trim() && !session.personalAgent; // the agent runs in home
  const blocked = noPath || session.busy;
  const [menuIdx, setMenuIdx] = useState(0);
  const [recording, setRecording] = useState(false);
  const [transcribing, setTranscribing] = useState(false);
  const [dictAvail, setDictAvail] = useState(false);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const mirrorRef = useRef<HTMLDivElement>(null); // highlight layer behind the textarea
  // Grow the input with its content (multi-line prompts), then scroll past a cap.
  useEffect(() => {
    const el = taRef.current; if (!el) return;
    el.style.height = "auto";
    el.style.height = Math.min(el.scrollHeight, 168) + "px";
  }, [session.draft]);
  useEffect(() => { invoke<boolean>("dictation_available").then(setDictAvail).catch(() => setDictAvail(false)); }, []);
  // 📎 attach: images go to vision, other files are referenced by path.
  const attach = async () => {
    const picked = await invoke<string[]>("pick_files").catch(() => [] as string[]);
    for (const f of picked) {
      const img = await invoke<boolean>("is_image", { path: f }).catch(() => false);
      if (img) {
        const url = await invoke<string | null>("read_image_b64", { path: f }).catch(() => null);
        if (url) onPatch((s) => ({ ...s, images: [...(s.images ?? []), { path: f, url }] }));
      } else onPatch((s) => ({ ...s, files: [...(s.files ?? []), f] }));
    }
  };
  // 🎤 push-to-talk dictation (offline faster-whisper): toggle record → transcribe.
  const mic = async () => {
    if (transcribing) return;
    if (!recording) { await invoke("start_dictation").then(() => setRecording(true)).catch(() => {}); return; }
    setRecording(false); setTranscribing(true);
    const t = await invoke<string>("stop_dictation").catch(() => "");
    setTranscribing(false);
    if (t) onPatch((s) => ({ ...s, draft: (s.draft ? s.draft.trimEnd() + " " : "") + t }));
  };
  const [agentSkills, setAgentSkills] = useState<AgentSkill[]>(agentSkillsCache.v ?? []);
  const [lecternSkills, setLecternSkills] = useState<SkillInfo[]>([]);
  useEffect(() => {
    if (agentSkillsCache.v) return;
    invoke<AgentSkill[]>("agent_skills").then((s) => { agentSkillsCache.v = s; setAgentSkills(s); }).catch(() => {});
  }, []);
  // Lectern's own learned/recorded skills for this repo — refetched when one is recorded.
  useEffect(() => {
    if (!session.path.trim()) { setLecternSkills([]); return; }
    invoke<SkillInfo[]>("skills", { path: session.path }).then(setLecternSkills).catch(() => setLecternSkills([]));
  }, [session.path, skillsVersion]);
  const slashOpen = !session.busy && session.draft.startsWith("/") && !session.draft.includes(" ") && !session.draft.includes("\n");
  const matches = slashOpen ? SLASH.filter((c) => c.cmd.startsWith(session.draft.toLowerCase())) : [];
  const q = slashOpen ? session.draft.slice(1).toLowerCase() : "";
  const lecternMatches = slashOpen ? lecternSkills.filter((s) => s.name.toLowerCase().includes(q)) : [];
  const agentMatches = slashOpen ? agentSkills.filter((s) => s.name.toLowerCase().includes(q)) : [];
  const idx = Math.min(menuIdx, Math.max(0, matches.length - 1));
  const useSkill = (s: AgentSkill) => { onPatch((x) => ({ ...x, draft: `Use your ${s.name} skill to ` })); setMenuIdx(0); };
  const useLecternSkill = (s: SkillInfo) => { onPatch((x) => ({ ...x, attachedSkill: s.name, attachedSkillGui: s.gui, draft: x.draft.startsWith("/") ? "" : x.draft })); setMenuIdx(0); };
  const pick = (c: SlashCmd) => { onPatch((s) => ({ ...s, draft: "" })); setMenuIdx(0); onCommand(c.ready ? c.id : `soon:${c.cmd}`); };
  const onKey = (e: React.KeyboardEvent) => {
    if (slashOpen && matches.length) {
      if (e.key === "ArrowDown") { e.preventDefault(); setMenuIdx((i) => Math.min(i + 1, matches.length - 1)); return; }
      if (e.key === "ArrowUp") { e.preventDefault(); setMenuIdx((i) => Math.max(i - 1, 0)); return; }
      if (e.key === "Tab") { e.preventDefault(); onPatch((s) => ({ ...s, draft: matches[idx].cmd + " " })); setMenuIdx(0); return; }
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); pick(matches[idx]); return; }
      if (e.key === "Escape") { e.preventDefault(); onPatch((s) => ({ ...s, draft: "" })); return; }
    }
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); onSend(); }
  };
  const addImage = (path: string, url: string) => onPatch((s) => ({ ...s, images: [...(s.images ?? []), { path, url }] }));
  const onPaste = (e: React.ClipboardEvent) => {
    const items = Array.from(e.clipboardData.items);
    const item = items.find((it) => it.type.startsWith("image/"));
    if (item) {
      e.preventDefault();
      const file = item.getAsFile();
      if (!file) return;
      const reader = new FileReader();
      reader.onload = () => {
        const url = reader.result as string;
        const b64 = url.split(",")[1] ?? "";
        const ext = (file.type.split("/")[1] || "png").replace("jpeg", "jpg").replace("svg+xml", "svg");
        invoke<string>("save_pasted_image", { data: b64, ext }).then((path) => addImage(path, url)).catch(() => {});
      };
      reader.readAsDataURL(file);
      return;
    }
    // WebKitGTK doesn't expose pasted images in the JS event — read the OS clipboard from Rust.
    if (!e.clipboardData.getData("text/plain")) {
      e.preventDefault();
      invoke<{ path: string; data_url: string } | null>("read_clipboard_image")
        .then((img) => { if (img) addImage(img.path, img.data_url); })
        .catch(() => {});
    }
  };
  const orb: React.CSSProperties = { marginLeft: "auto", flexShrink: 0, width: 36, height: 36, borderRadius: 999, border: "none", display: "inline-flex", alignItems: "center", justifyContent: "center", fontFamily: "inherit", lineHeight: 1 };
  return (
    <div style={{ flexShrink: 0, padding: "6px 20px 18px", background: "transparent" }}>
      <div style={{ maxWidth: COL, margin: "0 auto", position: "relative" }}>
        {slashOpen && (matches.length > 0 || lecternMatches.length > 0 || agentMatches.length > 0) && (
          <div style={{ position: "absolute", bottom: "calc(100% + 8px)", left: 0, right: 0, maxHeight: 360, overflow: "auto", background: "var(--panel)", border: "1px solid var(--bd)", borderRadius: 12, boxShadow: "0 18px 50px -16px rgba(0,0,0,.6)", zIndex: 30 }}>
            {matches.length > 0 && <div className="mono" style={{ padding: "8px 13px", fontSize: 10, color: "var(--fg3)", borderBottom: "1px solid var(--bd2)" }}>Lectern commands</div>}
            {matches.map((c, i) => (
              <div key={c.cmd} onMouseEnter={() => setMenuIdx(i)} onMouseDown={(e) => { e.preventDefault(); pick(c); }}
                style={{ display: "flex", alignItems: "center", gap: 12, padding: "9px 13px", cursor: "pointer", background: i === idx ? "var(--hov)" : "transparent", opacity: c.ready ? 1 : 0.55 }}>
                <span className="mono" style={{ fontSize: 13, fontWeight: 600, color: "var(--fg)", minWidth: 92 }}>{c.cmd}</span>
                <span style={{ fontSize: 13, color: "var(--fg2)", flex: 1 }}>{c.desc}</span>
                {!c.ready && <span className="mono" style={{ fontSize: 10, color: "var(--fg3)", border: "1px solid var(--bd)", borderRadius: 5, padding: "2px 6px" }}>soon</span>}
              </div>
            ))}
            {lecternMatches.length > 0 && <div style={{ padding: "8px 13px", fontSize: 11, fontWeight: 600, color: "var(--fg3)", borderTop: matches.length ? "1px solid var(--bd2)" : "none", borderBottom: "1px solid var(--bd2)" }}>Your skills</div>}
            {lecternMatches.map((s) => (
              <div key={s.name} onMouseDown={(e) => { e.preventDefault(); useLecternSkill(s); }}
                style={{ display: "flex", alignItems: "center", gap: 12, padding: "9px 13px", cursor: "pointer" }}>
                <span className="mono" style={{ fontSize: 12.5, fontWeight: 600, color: ACCENT, minWidth: 92, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{s.name}</span>
                <span style={{ fontSize: 12.5, color: "var(--fg2)", flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{s.description || "Learned skill"}</span>
                <span className="mono" style={{ fontSize: 10, color: ACCENT, border: "1px solid var(--bd)", borderRadius: 5, padding: "2px 6px", flexShrink: 0 }}>skill</span>
              </div>
            ))}
            {agentMatches.length > 0 && <div style={{ padding: "8px 13px", fontSize: 11, fontWeight: 600, color: "var(--fg3)", borderTop: (matches.length || lecternMatches.length) ? "1px solid var(--bd2)" : "none", borderBottom: "1px solid var(--bd2)" }}>Claude Code skills</div>}
            {agentMatches.map((s) => (
              <div key={s.name} onMouseDown={(e) => { e.preventDefault(); useSkill(s); }}
                style={{ display: "flex", alignItems: "center", gap: 12, padding: "9px 13px", cursor: "pointer" }}>
                <span className="mono" style={{ fontSize: 12.5, fontWeight: 600, color: "var(--fg)", minWidth: 92, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{s.name}</span>
                <span style={{ fontSize: 12.5, color: "var(--fg2)", flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{s.description || "Agent skill"}</span>
                <span className="mono" style={{ fontSize: 10, color: "var(--fg3)", border: "1px solid var(--bd)", borderRadius: 5, padding: "2px 6px", flexShrink: 0 }}>agent</span>
              </div>
            ))}
          </div>
        )}
        <div className="composer-pill" style={{ border: "1px solid var(--bd)", borderRadius: 18, background: "var(--panel)", padding: "14px 16px", display: "flex", flexDirection: "column", gap: 11 }}>
          {session.queued && (
            <div style={{ display: "flex" }}>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 8, maxWidth: "100%", background: "var(--hov)", border: "1px solid var(--bd)", color: "var(--fg2)", borderRadius: 8, padding: "5px 10px", fontSize: 12.5 }}>
                <span style={{ color: "var(--fg3)", flexShrink: 0 }}>queued next ·</span>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{session.queued}</span>
                <button onClick={() => onPatch((s) => ({ ...s, queued: undefined }))} title="cancel queued" style={{ border: "none", background: "transparent", color: "var(--fg3)", cursor: "pointer", lineHeight: 1, padding: 0, display: "inline-flex", flexShrink: 0 }}><Icon name="x" size={13} /></button>
              </span>
            </div>
          )}
          {session.attachedSkill && (
            <div style={{ display: "flex" }}>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 7, background: "var(--hov)", border: "1px solid var(--bd)", color: "var(--fg)", borderRadius: 8, padding: "5px 10px", fontSize: 12.5, fontWeight: 600 }}>
                {session.attachedSkill}{session.attachedSkillGui ? <span style={{ opacity: 0.6, fontWeight: 500 }}>· replays</span> : null}
                <button onClick={() => onPatch((s) => ({ ...s, attachedSkill: undefined, attachedSkillGui: undefined }))} title="remove" style={{ border: "none", background: "transparent", color: "var(--fg3)", cursor: "pointer", lineHeight: 1, padding: 0, display: "inline-flex" }}><Icon name="x" size={13} /></button>
              </span>
            </div>
          )}
          {(session.images?.length ?? 0) > 0 && (
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                {session.images!.map((im, i) => (
                  <div key={i} style={{ position: "relative", width: 56, height: 56, borderRadius: 8, overflow: "hidden", border: "1px solid var(--bd)" }}>
                    <img src={im.url} alt="" style={{ width: "100%", height: "100%", objectFit: "cover" }} />
                    <button onClick={() => onPatch((s) => ({ ...s, images: (s.images ?? []).filter((_, j) => j !== i) }))} title="remove"
                      style={{ position: "absolute", top: 2, right: 2, width: 16, height: 16, borderRadius: "50%", border: "none", background: "rgba(0,0,0,.7)", color: "#fff", lineHeight: 1, cursor: "pointer", padding: 0, display: "flex", alignItems: "center", justifyContent: "center" }}><Icon name="x" size={10} /></button>
                  </div>
                ))}
              </div>
              {/* Capability truth: images ride as file paths the agent must read. Claude and
                  Gemini read images; most free/local text models cannot, so say so up front. */}
              {!(isClaude || session.backend === "antigravity") && (
                <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 11.5, color: "var(--fg2)" }}>
                  <AlertTriangle size={12} strokeWidth={1.8} style={{ flexShrink: 0 }} />
                  This model may not be able to see images — they are passed as file paths. Claude and Gemini read them; most free or local models do not.
                </div>
              )}
            </div>
          )}
          {(session.files?.length ?? 0) > 0 && (
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
              {session.files!.map((f, i) => (
                <span key={i} className="mono" style={{ display: "inline-flex", alignItems: "center", gap: 6, background: "var(--panel2)", border: "1px solid var(--bd)", borderRadius: 8, padding: "4px 8px", fontSize: 11, color: "var(--fg2)", maxWidth: 220 }}>
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{f.split("/").pop()}</span>
                  <button onClick={() => onPatch((s) => ({ ...s, files: (s.files ?? []).filter((_, j) => j !== i) }))} title="remove" style={{ border: "none", background: "transparent", color: "var(--fg3)", cursor: "pointer", lineHeight: 1, padding: 0, display: "inline-flex" }}><Icon name="x" size={12} /></button>
                </span>
              ))}
            </div>
          )}
          <div style={{ position: "relative" }}>
            <div ref={mirrorRef} aria-hidden="true" style={{ position: "absolute", inset: 0, overflow: "hidden", pointerEvents: "none", color: "var(--fg)", fontSize: 14.5, lineHeight: "22px", padding: "1px 2px 0", fontFamily: "inherit", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{highlightDraft(session.draft)}{"\n"}</div>
            <textarea ref={taRef} value={session.draft} onChange={(e) => onPatch((s) => ({ ...s, draft: e.target.value }))}
              onKeyDown={onKey} onPaste={onPaste} onScroll={(e) => { if (mirrorRef.current) mirrorRef.current.scrollTop = e.currentTarget.scrollTop; }} rows={1}
              placeholder={session.busy ? "running… queue a follow-up (Enter), or Esc/Stop to cancel" : recording ? "listening… click the mic to stop" : transcribing ? "transcribing…" : personalAgent ? "Ask the agent anything…" : session.mode === "conduct" ? "Conductor is on — describe the task to plan & orchestrate…" : session.mode === "one-shot" ? "One-shot is on — give a brief; it builds autonomously…" : "Describe a task, or try a skill  (/ for commands)"}
              style={{ display: "block", position: "relative", border: "none", outline: "none", resize: "none", background: "transparent", color: "transparent", caretColor: "var(--fg)", fontSize: 14.5, lineHeight: "22px", padding: "1px 2px 0", width: "100%", fontFamily: "inherit", minHeight: 24, maxHeight: 168, overflowY: "auto" }} />
          </div>
          {session.mode && !personalAgent && (
            <div style={{ display: "flex" }}>
              <button
                className="lectern-fadein"
                onClick={() => onPatch((s) => ({ ...s, mode: undefined }))}
                title={session.mode === "conduct"
                  ? "Conductor mode is on — every send plans the task and hands sub-tasks to the best models. Click to turn off."
                  : "One-shot mode is on — every send runs as an autonomous build (auto-apply, skips permissions). Click to turn off."}
                style={{ display: "inline-flex", alignItems: "center", gap: 8, border: `1px solid ${session.mode === "one-shot" ? WARN : "var(--fg)"}`, background: "transparent", color: session.mode === "one-shot" ? WARN : "var(--fg)", borderRadius: 999, padding: "3px 12px", fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}
              >
                {session.mode} mode <span style={{ opacity: 0.55, fontWeight: 500 }}>on</span>
                <span style={{ opacity: 0.55, display: "inline-flex" }}><Icon name="x" size={12} /></span>
              </button>
            </div>
          )}
          {!session.draft && !session.busy && !personalAgent && !session.mode && (
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
              {SLASH.filter((c) => c.ready).slice(0, 3).map((c) => (
                <button key={c.cmd} onClick={() => c.id === "conduct" || c.id === "one-shot" ? onPatch((s) => ({ ...s, mode: c.id as "conduct" | "one-shot" })) : onPatch((s) => ({ ...s, draft: c.cmd + " " }))} title={c.desc}
                  style={{ border: "1px solid var(--bd)", background: "var(--panel2)", color: "var(--fg2)", borderRadius: 999, padding: "3px 11px", fontSize: 12, fontWeight: 500, cursor: "pointer", fontFamily: "inherit" }}>{c.cmd}</button>
              ))}
            </div>
          )}
          <div style={{ display: "flex", alignItems: "center" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 2 }}>
              <button className="icon-btn" onClick={attach} disabled={session.busy} title="Attach images or files"
                style={{ width: 32, height: 32, borderRadius: 9, border: "none", background: "transparent", color: "var(--fg3)", cursor: session.busy ? "default" : "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center", padding: 0 }}><Icon name="paperclip" size={18} /></button>
              {dictAvail && (
                <button className="icon-btn" onClick={mic} disabled={session.busy} title={recording ? "Stop & transcribe" : transcribing ? "Transcribing…" : "Dictate (voice)"}
                  style={{ width: 32, height: 32, borderRadius: 9, border: "none", background: "transparent", color: "var(--fg3)", cursor: session.busy ? "default" : "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center", padding: 0 }}>
                  {transcribing ? <span className="mono" style={{ fontSize: 13, color: ACCENT }}>…</span> : <span className={recording ? "mic-rec" : ""} style={{ display: "flex" }}><Icon name="mic" size={18} /></span>}
                </button>
              )}
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 4, marginLeft: "auto" }}>
              {!personalAgent && !session.busy && (
                <ScheduleControl session={session} onScheduled={(text) => onPatch((s) => ({ ...s, draft: "", events: [...s.events, { type: "message", text }] }))} />
              )}
              {!session.busy && (
                <ModelMenu backend={session.backend} model={session.model} backends={backends} models={models}
                  apply={session.apply} yolo={session.yolo} showRunMode={!personalAgent}
                  onModel={(b, m) => onPatch((s) => ({ ...s, backend: b, model: m }))}
                  onApply={() => onPatch((s) => ({ ...s, apply: !s.apply, yolo: s.apply ? false : s.yolo }))}
                  onYolo={() => onPatch((s) => (s.apply ? { ...s, yolo: !s.yolo } : s))} />
              )}
              {session.busy ? (
                <button className="send-orb" onClick={onCancel} title="Stop (Esc)" style={{ ...orb, marginLeft: 0, background: "var(--panel2)", color: "var(--fg)", border: "1px solid var(--bd)", cursor: "pointer" }}>
                  <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden><rect x="1.5" y="1.5" width="9" height="9" rx="2" fill="currentColor" /></svg>
                </button>
              ) : (
                <button className="send-orb" onClick={blocked ? undefined : onSend} disabled={blocked} title="Send (Enter)" style={{ ...orb, marginLeft: 0, background: blocked ? "var(--bd)" : "var(--btn)", color: blocked ? "var(--fg3)" : "var(--btnfg)", cursor: blocked ? "default" : "pointer", fontSize: 17, fontWeight: 700 }}>↑</button>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

// ── Profile ──────────────────────────────────────────────────────────────────
function Profile({ }: { onSignOutHint: () => void }) {
  const [acct, setAcct] = useState<AccountInfo | null>(null);
  useEffect(() => { invoke<AccountInfo>("account").then(setAcct).catch(() => setAcct({ signed_in: false, base_url: null, plan: null })); }, []);
  return (
    <Scroll>
      <div style={{ maxWidth: 620, margin: "0 auto", padding: "44px 40px", display: "flex", flexDirection: "column", gap: 24 }}>
        <div style={{ fontSize: 26, fontWeight: 800, letterSpacing: "-0.02em" }}>Profile</div>
        {acct === null ? (
          <div className="mono" style={{ fontSize: 12, color: "var(--fg3)" }}>Loading…</div>
        ) : acct.signed_in ? (
          <>
            <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
              <div style={{ width: 56, height: 56, borderRadius: "50%", border: "1px solid var(--bd)", background: "var(--panel2)", display: "flex", alignItems: "center", justifyContent: "center", fontWeight: 700, fontSize: 18 }}>L</div>
              <div><div style={{ fontSize: 20, fontWeight: 800 }}>Signed in</div><div className="mono" style={{ fontSize: 12, color: "var(--fg3)", marginTop: 2 }}>{acct.plan ? `${acct.plan} plan · ` : ""}{acct.base_url}</div></div>
            </div>
            <div className="mono" style={{ fontSize: 12, color: "var(--fg3)", lineHeight: 1.6 }}>Manage your plan, usage, and billing on the web dashboard. Sign out with <span style={{ color: "var(--fg)" }}>lectern logout</span>.</div>
          </>
        ) : (
          <div style={{ border: "1px solid var(--bd)", borderRadius: 13, background: "var(--panel)", padding: "30px 22px", textAlign: "center" }}>
            <div style={{ fontSize: 16, fontWeight: 700 }}>Not signed in</div>
            <div style={{ fontSize: 13.5, color: "var(--fg2)", marginTop: 8, lineHeight: 1.6, maxWidth: 420, margin: "8px auto 0" }}>Sign in to sync skills and see usage across devices. Run <span className="mono" style={{ color: "var(--fg)" }}>lectern login</span> in your terminal, then reopen this screen.</div>
          </div>
        )}
      </div>
    </Scroll>
  );
}

// ── shared bits ──────────────────────────────────────────────────────────────
export function Scroll({ children }: { children: React.ReactNode }) {
  return <div style={{ flex: 1, minHeight: 0, overflow: "auto" }}>{children}</div>;
}
export function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 13 }}>
      <div className="mono" style={{ fontSize: 11, color: "var(--fg3)" }}>{label}</div>
      {children}
    </div>
  );
}
const ICONS: Record<string, LucideIcon> = {
  paperclip: Paperclip, mic: Mic, send: ArrowUp, chevron: ChevronDown,
  home: Home, chat: MessageSquare, agent: Sparkles, market: LayoutGrid, usage: BarChart3,
  brain: BrainGlyph, schedule: Calendar, settings: SlidersHorizontal,
  profile: User, search: Search, folder: Folder, collapse: PanelLeftClose,
  panelLeft: PanelLeft, panelLeftClose: PanelLeftClose, panelRight: PanelRight,
  panelRightClose: PanelRightClose, newsession: SquarePen, branch: GitBranch,
  more: MoreHorizontal, x: X,
};
export function Icon({ name, size = 17 }: { name: string; size?: number }) {
  const C = ICONS[name];
  return C ? <C size={size} strokeWidth={1.8} style={{ display: "block", flexShrink: 0 }} /> : null;
}

// Composer control: pick the model (grouped by provider; only connected providers are
// selectable) AND the run mode (Plan / Apply / Autonomous) in one popover — one menu instead
// of two, so the composer stays uncluttered. The button shows the current model.
export function SunIcon({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <circle cx="12" cy="12" r="4.2" />
      <path d="M12 2.8v2.4M12 18.8v2.4M4.9 4.9l1.7 1.7M17.4 17.4l1.7 1.7M2.8 12h2.4M18.8 12h2.4M4.9 19.1l1.7-1.7M17.4 6.6l1.7-1.7" />
    </svg>
  );
}
export function MoonIcon({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M20.2 14.1A8.3 8.3 0 0 1 9.9 3.8a8.3 8.3 0 1 0 10.3 10.3Z" />
    </svg>
  );
}

/* Real switch control — replaces the "● on / ○ off" mono pills.
   Track inverts to fg when on (no green per design rules); danger variant tints. */
export function Switch({ on, onClick, danger, label }: { on: boolean; onClick: () => void; danger?: boolean; label?: string }) {
  const onBg = danger ? DANGER : "var(--fg)";
  return (
    <button role="switch" aria-checked={on} aria-label={label} onClick={onClick}
      style={{ display: "inline-flex", alignItems: "center", gap: 9, border: "none", background: "transparent", cursor: "pointer", padding: 0, fontFamily: "inherit" }}>
      {label && <span style={{ fontSize: 12, fontWeight: 600, color: on ? "var(--fg)" : "var(--fg3)" }}>{label}</span>}
      <span aria-hidden style={{ width: 36, height: 20, borderRadius: 999, position: "relative", flexShrink: 0, background: on ? onBg : "var(--panel2)", border: `1px solid ${on ? onBg : "var(--bd2)"}`, transition: "background .22s ease, border-color .22s ease" }}>
        <span style={{ position: "absolute", top: 2, left: 2, width: 14, height: 14, borderRadius: "50%", background: on ? "var(--bg)" : "var(--fg3)", transform: on ? "translateX(16px)" : "none", transition: "transform .26s cubic-bezier(.3,1.4,.4,1), background .22s ease" }} />
      </span>
    </button>
  );
}

/* Custom themed listbox — replaces every native <select>. Button +
   popover, grouped items, disabled rows, keyboard nav (arrows/Enter/Esc), backdrop
   close. Same visual language as ModelMenu. */
export function NiceSelect({ value, items, onPick, minWidth = 168, title }: {
  value: string;
  items: { id: string; label: string; group?: string; disabled?: boolean; hint?: string }[];
  onPick: (id: string) => void;
  minWidth?: number;
  title?: string;
}) {
  const [open, setOpen] = useState(false);
  const [hi, setHi] = useState(-1);
  const btn = useRef<HTMLButtonElement | null>(null);
  const cur = items.find((i) => i.id === value) ?? items[0];
  const enabled = items.map((i, idx) => ({ i, idx })).filter((x) => !x.i.disabled);
  const move = (dir: 1 | -1) => {
    if (enabled.length === 0) return;
    const pos = enabled.findIndex((x) => x.idx === hi);
    const next = pos < 0 ? (dir === 1 ? 0 : enabled.length - 1) : (pos + dir + enabled.length) % enabled.length;
    setHi(enabled[next].idx);
  };
  const key = (e: React.KeyboardEvent) => {
    if (!open) {
      if (e.key === "Enter" || e.key === " " || e.key === "ArrowDown") { e.preventDefault(); setOpen(true); setHi(items.findIndex((i) => i.id === value)); }
      return;
    }
    if (e.key === "Escape") { e.preventDefault(); setOpen(false); btn.current?.focus(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); move(1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); move(-1); }
    else if (e.key === "Enter" && hi >= 0 && !items[hi]?.disabled) { e.preventDefault(); onPick(items[hi].id); setOpen(false); btn.current?.focus(); }
  };
  const groups = [...new Set(items.map((i) => i.group ?? ""))];
  return (
    <div style={{ position: "relative", flexShrink: 0 }} onKeyDown={key}>
      <button ref={btn} onClick={() => setOpen((o) => !o)} title={title} aria-haspopup="listbox" aria-expanded={open}
        style={{ ...ctrl, minWidth, display: "inline-flex", alignItems: "center", justifyContent: "space-between", gap: 8, cursor: "pointer", fontFamily: "inherit" }}>
        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{cur?.label ?? "—"}</span>
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0, color: "var(--fg3)", transform: open ? "rotate(180deg)" : "none", transition: "transform .18s ease" }} aria-hidden><path d="m6 9 6 6 6-6" /></svg>
      </button>
      {open && (
        <>
          <div onClick={() => setOpen(false)} style={{ position: "fixed", inset: 0, zIndex: 60 }} />
          <div role="listbox" className="lectern-pop" style={{ position: "absolute", right: 0, top: "calc(100% + 6px)", zIndex: 61, minWidth: Math.max(minWidth, 210), maxHeight: 320, overflowY: "auto", background: "var(--bg)", border: "1px solid var(--bd)", borderRadius: 11, boxShadow: "0 18px 50px -12px rgba(0,0,0,.45)", padding: 5 }}>
            {groups.map((g) => (
              <div key={g || "_"}>
                {g && <div className="mono" style={{ fontSize: 9.5, fontWeight: 600, letterSpacing: "0.07em", color: "var(--fg3)", padding: "7px 9px 3px" }}>{g.toUpperCase()}</div>}
                {items.map((it, idx) => (it.group ?? "") === g && (
                  <div key={it.id} role="option" aria-selected={it.id === value}
                    onMouseEnter={() => !it.disabled && setHi(idx)}
                    onClick={() => { if (!it.disabled) { onPick(it.id); setOpen(false); } }}
                    style={{ display: "flex", alignItems: "center", gap: 8, padding: "7px 9px", borderRadius: 7, cursor: it.disabled ? "default" : "pointer", opacity: it.disabled ? 0.45 : 1, background: idx === hi && !it.disabled ? "var(--hov)" : "transparent", fontSize: 12.5 }}>
                    <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{it.label}</span>
                    {it.hint && <span className="mono" style={{ fontSize: 10, color: "var(--fg3)", flexShrink: 0 }}>{it.hint}</span>}
                    {it.id === value && <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }} aria-hidden><path d="m4.5 12.5 5 5 10-11" /></svg>}
                  </div>
                ))}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function ModelMenu({ backend, model, backends, models, apply, yolo, showRunMode, onModel, onApply, onYolo }: {
  backend: string; model: string; backends: BackendInfo[]; models: ModelOpt[];
  apply: boolean; yolo: boolean; showRunMode: boolean;
  onModel: (backend: string, model: string) => void; onApply: () => void; onYolo: () => void;
}) {
  const [open, setOpen] = useState(false);
  const avail = (b: string) => b === "auto" || (backends.find((x) => x.id === b)?.available ?? false);
  const cur = models.find((m) => m.backend === backend && (m.backend === "auto" || m.model === model)) ?? models[0];
  const short = cur.label.split(" — ")[0].replace(/^Claude /, "");
  const groups = [...new Set(models.map((m) => m.group))];
  const row = (m: ModelOpt) => {
    const ok = avail(m.backend);
    const on = m.id === cur.id;
    return (
      <button key={m.id} disabled={!ok} onClick={() => { onModel(m.backend, m.model); setOpen(false); }}
        style={{ display: "flex", alignItems: "center", gap: 8, width: "100%", padding: "7px 9px", borderRadius: 8, border: "none", background: on ? "var(--hov)" : "transparent", color: ok ? "var(--fg)" : "var(--fg3)", cursor: ok ? "pointer" : "default", fontFamily: "inherit", fontSize: 13, textAlign: "left", opacity: ok ? 1 : 0.6 }}>
        <span style={{ width: 12, color: ACCENT, flexShrink: 0 }}>{on ? "✓" : ""}</span>
        <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{m.label}</span>
        {!ok && <span style={{ fontSize: 10, color: "var(--fg3)", flexShrink: 0 }}>not connected</span>}
      </button>
    );
  };
  return (
    <div style={{ position: "relative", flexShrink: 0 }}>
      <button onClick={() => setOpen((o) => !o)} className="icon-btn" title="Model & run mode"
        style={{ display: "inline-flex", alignItems: "center", gap: 4, height: 30, padding: "0 9px", borderRadius: 8, border: "none", background: "transparent", color: "var(--fg2)", cursor: "pointer", fontFamily: "inherit", fontSize: 12.5, fontWeight: 600 }}>
        {short} <Icon name="chevron" size={13} />
      </button>
      {open && (
        <>
          <div onClick={() => setOpen(false)} style={{ position: "fixed", inset: 0, zIndex: 25 }} />
          <div style={{ position: "absolute", bottom: "calc(100% + 8px)", right: 0, width: 252, maxHeight: "62vh", overflow: "auto", background: "var(--panel)", border: "1px solid var(--bd)", borderRadius: 12, boxShadow: "0 18px 50px -16px rgba(0,0,0,.45)", padding: 6, zIndex: 26 }}>
            {groups.map((g) => g === "" ? models.filter((m) => m.group === g).map(row) : (
              <div key={g}>
                <div className="mono" style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 9.5, color: "var(--fg3)", padding: "6px 9px 2px", textTransform: "uppercase", letterSpacing: ".04em" }}>
                  {g.includes("Claude") ? <ClaudeIcon size={11} /> : g.includes("Antigravity") ? <AntigravityIcon size={11} /> : null}
                  {g}
                </div>
                {models.filter((m) => m.group === g).map(row)}
              </div>
            ))}
            {showRunMode && (
              <>
                <div style={{ height: 1, background: "var(--bd2)", margin: "6px 4px" }} />
                <div className="mono" style={{ fontSize: 9.5, color: "var(--fg3)", padding: "2px 8px 5px" }}>Run mode</div>
                <OptRow on={apply} label="Apply edits" desc="Write changes to your repo" onClick={onApply} />
                <OptRow on={yolo} label="Autonomous" desc="Run commands without asking" disabled={!apply} danger onClick={onYolo} />
              </>
            )}
            {/* honest source note: discovered from ~/.claude.json vs static fallback */}
            <div style={{ height: 1, background: "var(--bd2)", margin: "6px 4px" }} />
            <div className="mono" style={{ fontSize: 9, color: "var(--fg-ghost, var(--fg3))", padding: "2px 9px 4px", lineHeight: 1.5 }}>
              {models.some((m) => m.backend === "claude-code" && m.model.startsWith("claude-"))
                ? "Claude models from your account · refreshed on launch"
                : "standard model list — account discovery unavailable"}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
function OptRow({ on, label, desc, onClick, danger, disabled }: { on: boolean; label: string; desc: string; onClick: () => void; danger?: boolean; disabled?: boolean }) {
  const onColor = danger ? DANGER : "var(--fg)";
  return (
    <button onClick={onClick} disabled={disabled} style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 9px", borderRadius: 8, border: `1px solid ${on ? (danger ? DANGER : "var(--bd2)") : "var(--bd)"}`, background: on ? (danger ? "rgba(229,104,122,.10)" : "var(--hov)") : "transparent", cursor: disabled ? "default" : "pointer", textAlign: "left", fontFamily: "inherit", opacity: disabled ? 0.45 : 1, width: "100%" }}>
      <span style={{ width: 30, height: 17, borderRadius: 9, flexShrink: 0, background: on ? onColor : "var(--bd)", position: "relative" }}>
        <span style={{ position: "absolute", top: 2, left: on ? 15 : 2, width: 13, height: 13, borderRadius: "50%", background: "var(--bg)", transition: "left .15s ease" }} />
      </span>
      <span style={{ minWidth: 0 }}>
        <div style={{ fontSize: 12.5, fontWeight: 600, color: on ? onColor : "var(--fg)" }}>{label}</div>
        <div style={{ fontSize: 10.5, color: "var(--fg3)" }}>{desc}</div>
      </span>
    </button>
  );
}
const mi = { fill: "none" as const, stroke: "currentColor", strokeWidth: 1.7, strokeLinecap: "round" as const, strokeLinejoin: "round" as const, width: 12, height: 12, viewBox: "0 0 24 24" };
const BrainMini = <svg {...mi} aria-hidden><path d="M9.5 4.5a3 3 0 0 0-3 3c-1.7.4-3 1.9-3 3.8 0 1.4.7 2.6 1.8 3.3-.1.3-.1.6-.1 1a3.4 3.4 0 0 0 3.4 3.4c.9 0 1.8-.4 2.4-1V6a3 3 0 0 0-1.5-1.5ZM14.5 4.5a3 3 0 0 1 3 3c1.7.4 3 1.9 3 3.8 0 1.4-.7 2.6-1.8 3.3.1.3.1.6.1 1a3.4 3.4 0 0 1-3.4 3.4c-.9 0-1.8-.4-2.4-1V6a3 3 0 0 1 1.5-1.5Z" /></svg>;
const SparkMini = <svg {...mi} aria-hidden><path d="M12 3.5 13.8 9 19.5 11 13.8 13 12 18.5 10.2 13 4.5 11 10.2 9 12 3.5Z" /></svg>;
const RouteMini = <svg {...mi} aria-hidden><circle cx="5.5" cy="18.5" r="2" /><circle cx="18.5" cy="5.5" r="2" /><path d="M7.5 18.5H14a3.5 3.5 0 0 0 0-7H9.5a3.5 3.5 0 0 1 0-7h7" /></svg>;
const TermMini = <svg {...mi} aria-hidden><rect x="3" y="4.5" width="18" height="15" rx="2.5" /><path d="m7.5 9.5 3 3-3 3M12.5 15.5h4.5" /></svg>;
const HistoryMini = <svg {...mi} aria-hidden><path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 4v4h4" /><path d="M12 7.5V12l3 2" /></svg>;

/* Hermes-style file chip: extension label in a small tile (real file identity,
   no invented brand marks). */
function FileChip({ path }: { path: string }) {
  const ext = (path.split(".").pop() ?? "").toLowerCase().slice(0, 4);
  const label = ({ tsx: "TSX", ts: "TS", rs: "RS", py: "PY", md: "MD", css: "CSS", json: "{}", html: "<>", toml: "TML", yml: "YML", yaml: "YML", sh: "SH", js: "JS", jsx: "JSX" } as Record<string, string>)[ext];
  return (
    <span className="mono" style={{ fontSize: label && label.length > 2 ? 6.5 : 8, fontWeight: 700, letterSpacing: "0.02em" }}>
      {label ?? <svg {...mi} aria-hidden><path d="M13.5 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5L13.5 3Z" /><path d="M13.5 3v5.5H19" /></svg>}
    </span>
  );
}

function Dots() {
  return (
    <span className="lectern-dots" aria-hidden>
      <span /><span /><span />
    </span>
  );
}
function Thinking() {
  return (
    <div className="lectern-fadein" style={{ display: "flex", alignItems: "center", gap: 9, fontSize: 13.5, lineHeight: 1.5 }}>
      <Dots />
      <span className="lectern-think">Thinking</span>
    </div>
  );
}
// The engine streams running token totals (Usage events); the latest one is the
// current cumulative count. Returns 0 until the first usage arrives.
function runningTokens(events: Ev[]): number {
  for (let i = events.length - 1; i >= 0; i--) {
    const e = events[i];
    if (e.type === "usage") {
      const u = e as { input_tokens: number; output_tokens: number };
      return (u.input_tokens ?? 0) + (u.output_tokens ?? 0);
    }
  }
  return 0;
}
function Working({ tokens }: { tokens?: number }) {
  if (!tokens || tokens <= 0) return <Thinking />;
  return (
    <div className="lectern-fadein" style={{ display: "flex", alignItems: "center", gap: 9, fontSize: 13.5, lineHeight: 1.5 }}>
      <Dots />
      <span className="lectern-think">Thinking</span>
      <span className="mono" style={{ fontSize: 11, color: "var(--fg3)" }}>{tokens.toLocaleString()} tokens</span>
    </div>
  );
}

// A quiet, collapsed-by-default row for agent "machinery" (tool calls, diffs, memory,
// routing). Shows a one-line summary; click to expand the detail. Keeps the final answer
// prominent while the process folds away.
function Collapsible({ summary, mono, icon, children }: { summary: React.ReactNode; mono?: boolean; icon?: React.ReactNode; children?: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  const hasDetail = !!children;
  return (
    <div>
      <button className="lectern-row" onClick={() => hasDetail && setOpen((o) => !o)} disabled={!hasDetail}
        style={{ display: "inline-flex", alignItems: "center", gap: 8, maxWidth: "100%", padding: "3px 8px", margin: "-3px -8px", borderRadius: 7, background: "transparent", border: "none", color: "var(--fg3)", cursor: hasDetail ? "pointer" : "default", textAlign: "left", fontFamily: "inherit", fontSize: 12.5 }}>
        {icon && <span style={{ width: 20, height: 20, flexShrink: 0, border: "1px solid var(--bd2)", borderRadius: 6, display: "inline-flex", alignItems: "center", justifyContent: "center", color: "var(--fg2)" }}>{icon}</span>}
        <span className="lectern-chev" style={{ flexShrink: 0, fontSize: 10, transform: open ? "rotate(90deg)" : "none", opacity: hasDetail ? 0.65 : 0 }}>▸</span>
        <span className={mono ? "mono" : undefined} style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{summary}</span>
      </button>
      {open && hasDetail ? <div className="lectern-fadein" style={{ padding: "6px 0 4px 18px" }}>{children}</div> : null}
    </div>
  );
}
function SummaryView({ s }: { s: RunSummary }) {
  return (
    <div className="mono" style={{ fontSize: 11, color: "var(--fg3)", borderTop: "1px dashed var(--bd)", paddingTop: 12, marginTop: 2, display: "flex", gap: 18, flexWrap: "wrap" }}>
      <span>{s.changes} file change{s.changes === 1 ? "" : "s"}{s.applied ? " · applied" : s.changes ? " · review" : ""}</span>
      <span>{s.input_tokens.toLocaleString()} in / {s.output_tokens.toLocaleString()} out tokens</span>
      {s.limit_hit && <span style={{ color: WARN }}>usage limit hit — would fall back</span>}
    </div>
  );
}

// Pull image file paths (absolute, ~/, or ./) out of a tool command/output so the
// UI can show images the agent produced or viewed (e.g. screenshots).
function imagePaths(text: string): string[] {
  const re = /((?:\/|~\/|\.\/)[^\s"'`)]+\.(?:png|jpe?g|gif|webp|bmp))/gi;
  const out: string[] = [];
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) if (!out.includes(m[1])) out.push(m[1]);
  return out;
}
function AgentImage({ path }: { path: string }) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let ok = true;
    invoke<string | null>("read_image_b64", { path }).then((u) => { if (ok) setUrl(u); }).catch(() => {});
    return () => { ok = false; };
  }, [path]);
  if (!url) return null;
  return <img src={url} alt={path} title={path} style={{ display: "block", maxWidth: "100%", maxHeight: 360, borderRadius: 8, border: "1px solid var(--bd)", marginTop: 8 }} />;
}

/* Smooth streaming — backend chunks arrive in bursts, which reads as clunky
   text jumps. Render catches up per animation frame instead (rate scales with
   backlog so it never falls behind). Reduced motion mirrors chunks directly. */
function useSmoothText(target: string, live: boolean): string {
  const [shown, setShown] = useState(target);
  const targetRef = useRef(target);
  targetRef.current = target;
  const [reduced] = useState(() => !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches);
  useEffect(() => {
    if (!live || reduced) return;
    let raf = 0;
    const tick = () => {
      setShown((prev) => {
        const t = targetRef.current;
        if (prev === t) return prev;
        if (!t.startsWith(prev)) return t; // content replaced — snap
        const backlog = t.length - prev.length;
        const step = Math.min(16, Math.max(2, Math.ceil(backlog / 14)));
        return t.slice(0, prev.length + step);
      });
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [live, reduced]);
  return live && !reduced ? shown : target;
}

/* While streaming, the current (unfinished) paragraph renders as raw text with
   the trailing characters fading in individually — smooth, letter-level motion
   that stays fast. Completed paragraphs (and the final message) render as full
   Markdown. Char spans are keyed by absolute offset so each mounts exactly once. */
const CHAR_TAIL = 26;
function StreamedMessage({ text, live }: { text: string; live: boolean }) {
  const shown = useSmoothText(text, live);
  if (!live) {
    return (
      <span>
        <Markdown text={text} />
      </span>
    );
  }
  const brk = shown.lastIndexOf("\n\n");
  const paraStart = brk === -1 ? 0 : brk + 2;
  const stable = shown.slice(0, paraStart);
  const para = shown.slice(paraStart);
  const cut = Math.max(0, para.length - CHAR_TAIL);
  const paraHead = para.slice(0, cut);
  const tail = para.slice(cut);
  return (
    <span>
      {stable ? <Markdown text={stable} /> : null}
      <span style={{ whiteSpace: "pre-wrap" }}>
        {paraHead}
        {Array.from(tail).map((c, i) => (
          <span key={paraStart + cut + i} className="lectern-charin">
            {c}
          </span>
        ))}
      </span>
      <span className="lectern-caret" />
    </span>
  );
}

/* Memoized: pushEv only recreates the streaming (last) event object, so during
   60fps streaming every settled row keeps its reference and skips re-render —
   only the live row pays. */
// A rewind point in the timeline: the workspace snapshot taken before this turn wrote to
// disk. "Restore" asks for confirmation inline, then reverts the workspace to this snapshot.
function CheckpointMarker({ id, label, onRewind }: { id: string; label: string; onRewind?: (id: string, label: string) => void }) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const link: React.CSSProperties = { border: "none", background: "transparent", color: "var(--fg2)", cursor: "pointer", font: "inherit", padding: 0, textDecoration: "underline", textUnderlineOffset: 2 };
  return (
    <div className="mono" style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11.5, color: "var(--fg3)", padding: "1px 0" }}>
      <span style={{ display: "inline-flex", flexShrink: 0 }}>{HistoryMini}</span>
      <span>Checkpoint</span>
      <span style={{ color: "var(--fg2)" }}>{id}</span>
      {onRewind && !confirming && <button onClick={() => setConfirming(true)} style={link}>Restore</button>}
      {onRewind && confirming && (
        <span style={{ display: "inline-flex", gap: 8, alignItems: "center" }}>
          <span>restore this snapshot?</span>
          <button disabled={busy} onClick={async () => { setBusy(true); try { await onRewind(id, label); } finally { setBusy(false); setConfirming(false); } }} style={{ ...link, color: "var(--fg)" }}>{busy ? "restoring…" : "yes, restore"}</button>
          <button disabled={busy} onClick={() => setConfirming(false)} style={link}>cancel</button>
        </span>
      )}
    </div>
  );
}

export const EventView = memo(function EventView({ ev, live, onRetry, onRewind }: { ev: Ev; live?: boolean; onRetry?: () => void; onRewind?: (id: string, label: string) => void }) {
  switch (ev.type) {
    case "user": {
      const imgs = (ev as any).images as string[] | undefined;
      const oneShot = (ev as any).oneShot as boolean | undefined;
      const skill = (ev as any).skill as string | undefined;
      return (
        <div style={{ alignSelf: "flex-end", maxWidth: "82%", background: "var(--panel2)", border: "1px solid var(--bd)", borderRadius: "13px 13px 4px 13px", padding: "10px 14px", fontSize: 14, lineHeight: 1.5, display: "flex", flexDirection: "column", gap: 8 }}>
          {oneShot ? <span className="mono" style={{ alignSelf: "flex-start", fontSize: 10, color: WARN, border: `1px solid ${WARN}`, borderRadius: 5, padding: "2px 7px" }}>one-shot build</span> : null}
          {skill ? <span style={{ alignSelf: "flex-start", fontSize: 11.5, fontWeight: 600, color: ACCENT, border: `1px solid ${ACCENT}`, borderRadius: 6, padding: "2px 8px" }}>{skill}</span> : null}
          {imgs?.length ? (
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
              {imgs.map((u, i) => <img key={i} src={u} alt="" style={{ maxWidth: 180, maxHeight: 180, borderRadius: 8, border: "1px solid var(--bd)" }} />)}
            </div>
          ) : null}
          {(ev as any).text ? <span style={{ whiteSpace: "pre-wrap" }}>{(ev as any).text}</span> : null}
        </div>
      );
    }
    case "thinking":
      return <Thinking />;
    case "thought": {
      const recalls = (ev as any).recalls ?? [];
      if (recalls.length === 0) return <div className="mono" style={{ fontSize: 12, color: "var(--fg3)", lineHeight: 1.55 }}>{(ev as any).summary}</div>;
      return (
        <Collapsible icon={BrainMini} summary={<>Memory · {(ev as any).summary}</>}>
          <div className="mono" style={{ fontSize: 11.5, color: "var(--fg2)", display: "flex", flexDirection: "column", gap: 3 }}>
            {recalls.map((r: string, i: number) => <div key={i} style={{ display: "flex", gap: 7 }}><span style={{ color: "var(--fg3)", flexShrink: 0 }}>↳</span><span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r}</span></div>)}
          </div>
        </Collapsible>
      );
    }
    case "skill_applied":
      return (
        <Collapsible icon={SparkMini} summary={<>Skill · {(ev as any).name}</>}>
          <div style={{ fontSize: 12.5, color: "var(--fg2)", lineHeight: 1.5 }}>{(ev as any).why}</div>
        </Collapsible>
      );
    case "checkpoint":
      return <CheckpointMarker id={String((ev as any).id)} label={String((ev as any).label ?? "")} onRewind={onRewind} />;
    case "model_routed": {
      const m = String((ev as any).model || "default");
      const label = ({ opus: "Opus 4.8", sonnet: "Sonnet 4.6", haiku: "Haiku 4.5" } as Record<string, string>)[m] || m;
      return (
        <Collapsible icon={RouteMini} summary={<>Routed to {label}</>}>
          <div style={{ fontSize: 12.5, color: "var(--fg2)", lineHeight: 1.5 }}>{(ev as any).reason}</div>
        </Collapsible>
      );
    }
    case "plan":
      return (
        <div style={{ border: "1px solid var(--bd)", borderRadius: 10, padding: "13px 15px", display: "flex", flexDirection: "column", gap: 7, background: "var(--panel)" }}>
          <div style={{ fontSize: 12.5, fontWeight: 700, color: "var(--fg2)", letterSpacing: "-0.01em" }}>Plan</div>
          {(ev as any).steps.map((st: any, i: number) => (
            <div key={i} style={{ display: "flex", gap: 8, fontSize: 13.5, lineHeight: 1.5, color: st.done ? "var(--fg)" : "var(--fg2)", transition: "color .3s ease" }}><span style={{ color: st.done ? ACCENT : "var(--fg3)", transition: "color .3s ease" }}>{st.done ? "✓" : "•"}</span>{st.text}</div>
          ))}
        </div>
      );
    case "file_edit":
      return (
        <Collapsible
          mono
          icon={<FileChip path={String((ev as any).path ?? "")} />}
          summary={<span style={{ display: "inline-flex", gap: 10 }}><span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>Edited {(ev as any).path}</span><span style={{ flexShrink: 0, opacity: 0.8 }}>+{(ev as any).added} −{(ev as any).removed}</span></span>}
        >
          {((ev as any).preview ?? []).length > 0 ? (
            <div className="mono" style={{ background: "var(--elev)", border: "1px solid var(--bd2)", borderRadius: 8, padding: "8px 0", maxHeight: 280, overflow: "auto", lineHeight: 1.5, fontSize: 12 }}>
              {(ev as any).preview.map((l: any, i: number) => (
                <div key={i} style={{ padding: "0 11px", whiteSpace: "pre", color: l.kind === "add" ? "var(--diffAddFg)" : l.kind === "remove" ? "var(--diffRmFg)" : "var(--fg2)", background: l.kind === "add" ? "var(--diffAddBg)" : l.kind === "remove" ? "var(--diffRmBg)" : "transparent" }}>{l.kind === "add" ? "+ " : l.kind === "remove" ? "− " : "  "}{l.text}</div>
              ))}
            </div>
          ) : null}
        </Collapsible>
      );
    case "terminal": {
      const err = (ev as any).exit_code !== 0;
      const imgs = imagePaths(`${(ev as any).command} ${(ev as any).output ?? ""}`);
      const out = (ev as any).output as string | undefined;
      return (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <Collapsible
            mono
            icon={TermMini}
            summary={<span style={{ color: err ? "#c98f96" : "var(--fg3)" }}>$ {(ev as any).command}</span>}
          >
            {out ? (
              <div style={{ position: "relative" }}>
                <div className="mono" style={{ background: "#0b0b0d", border: "1px solid var(--bd2)", borderRadius: 8, padding: "9px 11px", fontSize: 12, lineHeight: 1.5, maxHeight: 280, overflow: "auto", color: err ? "#e58a97" : "#cfcfca", whiteSpace: "pre-wrap" }}>{out}</div>
                <CopyBtn text={out} />
              </div>
            ) : null}
          </Collapsible>
          {imgs.map((p) => <AgentImage key={p} path={p} />)}
        </div>
      );
    }
    case "message": {
      const txt = (ev as any).text as string;
      if (live) return <StreamedMessage text={txt} live />;
      return (
        <div className="msg-wrap" style={{ position: "relative" }}>
          <StreamedMessage text={txt} live={false} />
          <MsgCopy text={txt} />
        </div>
      );
    }
    case "limit_hit":
      return <div style={{ fontSize: 13, lineHeight: 1.5, color: WARN, border: "1px solid var(--bd)", background: "var(--panel)", borderRadius: 9, padding: "10px 13px" }}>Usage limit: {(ev as any).reason} — Lectern would fall back to the next backend.</div>;
    case "error":
      return (
        <div style={{ fontSize: 13, lineHeight: 1.5, color: DANGER, border: "1px solid var(--bd)", background: "var(--panel)", borderRadius: 9, padding: "10px 13px", whiteSpace: "pre-wrap" }}>
          error: {(ev as any).message}
          {onRetry && (
            <div style={{ marginTop: 8 }}>
              <button onClick={onRetry} title="Put the last prompt back in the composer to adjust and resend"
                style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 26, padding: "0 10px", borderRadius: 7, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg2)", fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>
                <RotateCcw size={12} strokeWidth={1.8} /> Edit &amp; retry
              </button>
            </div>
          )}
        </div>
      );
    default:
      return null;
  }
});

// Hover copy for a prose message — theme-toned (the dark CopyBtn is for code).
function MsgCopy({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      className="msg-copy icon-btn"
      onClick={() => { navigator.clipboard?.writeText(text); setDone(true); setTimeout(() => setDone(false), 1200); }}
      title="Copy message"
      style={{ position: "absolute", top: -2, right: 0, width: 24, height: 24, borderRadius: 6, border: "none", background: "transparent", color: done ? "var(--fg)" : "var(--fg3)", cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center", padding: 0 }}>
      {done ? <Check size={13} strokeWidth={1.8} /> : <Copy size={13} strokeWidth={1.8} />}
    </button>
  );
}

function Logo() {
  return (
    <div style={{ width: 22, height: 22, border: "1.5px solid var(--fg)", borderRadius: 4, display: "flex", alignItems: "center", justifyContent: "center", position: "relative" }}>
      <div style={{ width: 2, height: 11, background: "var(--fg)" }} />
      <div style={{ position: "absolute", bottom: 3, width: 11, height: 2, background: "var(--fg)" }} />
    </div>
  );
}

// ── lightweight markdown renderer (Claude output is markdown) ─────────────────
function inlineMd(text: string, kp: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  const re = /(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*]+\*)|(\[[^\]]+\]\([^)]+\))/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let i = 0;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) nodes.push(text.slice(last, m.index));
    const tok = m[0];
    if (tok.startsWith("`")) {
      nodes.push(<code key={`${kp}-${i}`} className="mono" style={{ background: "var(--chrome)", border: "1px solid var(--bd2)", borderRadius: 5, padding: "1px 5px", fontSize: "0.88em" }}>{tok.slice(1, -1)}</code>);
    } else if (tok.startsWith("**")) {
      nodes.push(<strong key={`${kp}-${i}`}>{tok.slice(2, -2)}</strong>);
    } else if (tok.startsWith("*")) {
      nodes.push(<em key={`${kp}-${i}`}>{tok.slice(1, -1)}</em>);
    } else {
      const mm = /\[([^\]]+)\]\(([^)]+)\)/.exec(tok);
      if (mm) nodes.push(<a key={`${kp}-${i}`} href={mm[2]} style={{ color: "var(--fg)", textDecoration: "underline", textUnderlineOffset: 2 }}>{mm[1]}</a>);
    }
    last = m.index + tok.length;
    i++;
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes;
}

// Hover copy button for code blocks — one click puts the agent's code on the
// clipboard (same path as the Share button) with brief "Copied" feedback.
function CopyBtn({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      onClick={() => { navigator.clipboard?.writeText(text); setDone(true); setTimeout(() => setDone(false), 1200); }}
      title="Copy to clipboard"
      className="mono"
      style={{ position: "absolute", top: 6, right: 6, height: 22, padding: "0 8px", borderRadius: 6, border: "1px solid rgba(255,255,255,.14)", background: "rgba(0,0,0,.4)", color: "#cfcfca", fontSize: 10.5, cursor: "pointer", opacity: 0.8 }}
    >
      {done ? "Copied" : "Copy"}
    </button>
  );
}
function Markdown({ text }: { text: string }) {
  const lines = text.split("\n");
  const blocks: React.ReactNode[] = [];
  let i = 0;
  let k = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.trim().startsWith("```")) {
      const buf: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith("```")) { buf.push(lines[i]); i++; }
      i++;
      const code = buf.join("\n");
      blocks.push(
        <div key={k++} style={{ position: "relative", margin: "6px 0" }}>
          <pre className="mono" style={{ background: "#0b0b0d", border: "1px solid var(--bd)", borderRadius: 8, padding: "10px 12px", overflow: "auto", fontSize: 12.5, lineHeight: 1.5, margin: 0, color: "#cfcfca", whiteSpace: "pre" }}>{code}</pre>
          <CopyBtn text={code} />
        </div>,
      );
      continue;
    }
    const h = /^(#{1,3})\s+(.*)$/.exec(line);
    if (h) {
      const size = h[1].length === 1 ? 19 : h[1].length === 2 ? 16.5 : 15;
      blocks.push(<div key={k++} style={{ fontWeight: 700, fontSize: size, margin: "12px 0 2px", letterSpacing: "-0.01em" }}>{inlineMd(h[2], `h${k}`)}</div>);
      i++;
      continue;
    }
    if (/^\s*([-*]|\d+\.)\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[i])) { items.push(lines[i].replace(/^\s*([-*]|\d+\.)\s+/, "")); i++; }
      blocks.push(<ul key={k++} style={{ margin: "5px 0", paddingLeft: 20, display: "flex", flexDirection: "column", gap: 3 }}>{items.map((it, j) => <li key={j} style={{ lineHeight: 1.55 }}>{inlineMd(it, `li${k}-${j}`)}</li>)}</ul>);
      continue;
    }
    if (line.trim() === "") { i++; continue; }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() !== "" && !lines[i].trim().startsWith("```") && !/^(#{1,3})\s+/.test(lines[i]) && !/^\s*([-*]|\d+\.)\s+/.test(lines[i])) { para.push(lines[i]); i++; }
    blocks.push(<p key={k++} style={{ margin: "4px 0", lineHeight: 1.65, whiteSpace: "pre-wrap" }}>{inlineMd(para.join("\n"), `p${k}`)}</p>);
  }
  return <div style={{ fontSize: 14.5, color: "var(--fg)" }}>{blocks}</div>;
}
