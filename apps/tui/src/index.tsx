/* lectern tui v2 — chat-core parity with the desktop app.
   Layout: session sidebar · chat pane with the app's event anatomy · status bar.
   Composer: model picker overlay (Ctrl+P), sticky /conduct + /one-shot modes,
   Esc cancels. Read-only Brain (Ctrl+B) and Skills (Ctrl+K) panels.
   `--once "<prompt>"` stays headless for scripting/verification. */
import { createCliRenderer } from "@opentui/core";
import { createRoot, useKeyboard } from "@opentui/react";
import { useEffect, useRef, useState } from "react";
import {
  brainStats, cancelRun, ensureDaemon, listModels, listSessions, listSkills, mcpOverview, pinSession,
  renameSession, runSession, sessionHistory, usageStats, type AgentEvent, type ModelInfo, type RunResult, type SessionMeta,
} from "./daemon";

// ── the app's dark palette, verbatim tokens ──────────────────────────────────
// C is mutated by the theme dialog (Object.assign) + a version bump re-renders.
const C = {
  bg: "#0f0f10", panel: "#161618", panel2: "#1d1d20", bd: "#2a2a2e", bd2: "#333338",
  fg: "#f4f4f2", fg2: "#b9b9b4", fg3: "#77776f", accent: "#7fd4a0", warn: "#e0b34d", danger: "#e5687a",
};
const BUILTIN_THEME = { ...C };
// desktop theme files (~/.lectern/themes/*.json, lectern_theme:1) → TUI palette
const THEME_VAR_MAP: Record<string, keyof typeof C> = {
  "--bg": "bg", "--panel": "panel", "--panel2": "panel2", "--bd": "bd", "--bd2": "bd2",
  "--fg": "fg", "--fg2": "fg2", "--fg3": "fg3", "--accent": "accent",
};
type TuiConfig = { theme?: string | null; backend?: string; model?: string; model_label?: string; clean?: boolean };
function tuiConfigPath(): string {
  const home = process.env.HOME ?? process.env.USERPROFILE ?? ".";
  return `${home}/.lectern/tui.json`;
}
function loadTuiConfig(): TuiConfig {
  try {
    const fs = require("node:fs") as typeof import("node:fs");
    return JSON.parse(fs.readFileSync(tuiConfigPath(), "utf8")) as TuiConfig;
  } catch { return {}; }
}
function saveTuiConfig(patch: TuiConfig): void {
  try {
    const fs = require("node:fs") as typeof import("node:fs");
    const path = tuiConfigPath();
    fs.mkdirSync(path.slice(0, path.lastIndexOf("/")), { recursive: true });
    fs.writeFileSync(path, JSON.stringify({ ...loadTuiConfig(), ...patch }, null, 2) + "\n");
  } catch { /* best-effort */ }
}
const CFG = loadTuiConfig();

function themesDir(): string {
  const home = process.env.HOME ?? process.env.USERPROFILE ?? ".";
  return `${home}/.lectern/themes`;
}
function listThemeFiles(): { file: string; name: string; base: string }[] {
  try {
    const fs = require("node:fs") as typeof import("node:fs");
    return fs.readdirSync(themesDir())
      .filter((f) => f.endsWith(".json"))
      .flatMap((f) => {
        try {
          const v = JSON.parse(fs.readFileSync(`${themesDir()}/${f}`, "utf8"));
          if (v?.lectern_theme !== 1) return [];
          return [{ file: f, name: String(v.name ?? f), base: String(v.base ?? "dark") }];
        } catch { return []; }
      });
  } catch { return []; }
}
function applyThemeFile(file: string | null): boolean {
  Object.assign(C, BUILTIN_THEME);
  if (!file) return true;
  try {
    const fs = require("node:fs") as typeof import("node:fs");
    const v = JSON.parse(fs.readFileSync(`${themesDir()}/${file}`, "utf8"));
    const vars = (v?.vars ?? {}) as Record<string, string>;
    for (const [k, key] of Object.entries(THEME_VAR_MAP)) {
      const val = vars[k];
      if (typeof val === "string" && /^#[0-9a-fA-F]{3,8}$/.test(val)) (C as Record<string, string>)[key] = val;
    }
    return true;
  } catch { return false; }
}

// ── args ──────────────────────────────────────────────────────────────────────
function arg(name: string, def = ""): string {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : def;
}
const flag = (name: string) => process.argv.includes(`--${name}`);
const TUI_VERSION = "3.0.0";
if (flag("version")) {
  console.log(`lectern-tui ${TUI_VERSION}`);
  process.exit(0);
}
const OPTS = {
  path: arg("path", process.cwd()),
  backend: arg("backend", "auto"),
  model: arg("model") || undefined,
  apply: flag("apply"),
  yolo: flag("yolo"),
};

// ── event → line mapping (the app's machinery anatomy) ───────────────────────
type Line = { kind: "user" | "dim" | "text" | "plan" | "add" | "rm" | "diff" | "term" | "err" | "sum"; text: string };
function eventLines(ev: AgentEvent): Line[] {
  switch (ev.type) {
    case "thinking": return [];
    case "user": return [{ kind: "user", text: `you · ${ev.text ?? ""}` }];
    case "message": return [{ kind: "text", text: String(ev.text ?? "") }];
    case "thought": {
      const recalls = (ev.recalls as string[]) ?? [];
      return [{ kind: "dim", text: recalls.length ? `✓ recalled ${recalls.length} file(s): ${recalls.slice(0, 3).join(", ")}` : String(ev.summary ?? "") }];
    }
    case "skill_applied": return [{ kind: "dim", text: `✦ skill · ${ev.name}` }];
    case "model_routed": return [{ kind: "dim", text: `⇄ routed to ${ev.model} — ${ev.reason}` }];
    case "plan": {
      const steps = (ev.steps as { done: boolean; text: string }[]) ?? [];
      return [{ kind: "plan", text: "Plan" }, ...steps.map((s) => ({ kind: "plan" as const, text: `  ${s.done ? "✓" : "•"} ${s.text}` }))];
    }
    case "file_edit": {
      const prev = ((ev.preview as { kind: string; text: string }[]) ?? []).slice(0, 6);
      return [
        { kind: "diff", text: `✎ ${ev.path}  +${ev.added} −${ev.removed}` },
        ...prev.map((l) => ({ kind: (l.kind === "add" ? "add" : l.kind === "remove" ? "rm" : "diff") as Line["kind"], text: `  ${l.kind === "add" ? "+" : l.kind === "remove" ? "−" : " "} ${l.text}` })),
      ];
    }
    case "terminal": {
      const out = String(ev.output ?? "").split("\n").slice(0, 4);
      return [{ kind: "term", text: `$ ${ev.command}` }, ...out.filter(Boolean).map((o) => ({ kind: "term" as const, text: `  ${o}` }))];
    }
    case "limit_hit": return [{ kind: "err", text: `usage limit: ${ev.reason}` }];
    case "error": return [{ kind: "err", text: String(ev.message ?? "error") }];
    case "usage": return [{ kind: "sum", text: `· ${ev.input_tokens} in / ${ev.output_tokens} out tokens` }];
    default: return [];
  }
}
const lineColor = (k: Line["kind"]) =>
  k === "user" ? C.fg : k === "err" ? C.danger : k === "text" ? "#dededa" :
  k === "add" ? "#9fe0ad" : k === "rm" ? "#e58a97" : k === "sum" ? C.fg3 :
  k === "term" ? "#9a9a96" : k === "plan" ? C.fg2 : "#9a9a96";

// ── headless --once (verification + scripting) ───────────────────────────────
const once = arg("once");
if (once) {
  if (!(await ensureDaemon())) { console.error("lecternd unreachable — start it with `lecternd`"); process.exit(1); }
  const conduct = once.startsWith("/conduct ");
  const prompt = conduct ? once.slice(9) : once;
  let stream = "";
  const res = await runSession({ ...OPTS, prompt, conduct }, (id) => console.log(`run ${id}`), (ev) => {
    if (ev.type === "message_delta") { stream += String(ev.text ?? ""); return; }
    if (ev.type === "message") { console.log(String(ev.text ?? stream)); stream = ""; return; }
    if (ev.type === "done" && stream) { console.log(stream); stream = ""; }
    for (const l of eventLines(ev)) console.log(l.text);
  });
  console.log(res.error ? `error: ${res.error}` : `done · ${res.changes} change(s) · ${res.input_tokens} in / ${res.output_tokens} out`);
  process.exit(res.error ? 1 : 0);
}

// configured theme applies before first render (CLI has no theme flag)
if (CFG.theme) applyThemeFile(CFG.theme);

// ── the TUI (v3 layout — OpenCode-style single-focus shell) ───────
type DialogItem = { id: string; label: string; hint?: string; live?: boolean };

function App() {
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [activeSid, setActiveSid] = useState<string | null>(null);
  const [activeTitle, setActiveTitle] = useState<string>("new session");
  const [lines, setLines] = useState<Line[]>([]);
  const [stream, setStream] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("ready");
  const [runMode, setRunMode] = useState<"plan" | "apply" | "one-shot" | "conduct">("plan");
  const [backend, setBackend] = useState(OPTS.backend !== "auto" ? OPTS.backend : (CFG.backend ?? "auto"));
  const [model, setModel] = useState<string | undefined>(OPTS.model ?? CFG.model ?? undefined);
  const [modelLabel, setModelLabel] = useState(OPTS.model ?? (CFG.model ? (CFG.model_label ?? CFG.model) : "auto"));
  const [panel, setPanel] = useState<null | { title: string; rows: string[] }>(null);
  const [, setThemeV] = useState(0); // bump after mutating C so styles re-read it
  const [gen, setGen] = useState(0);
  const [chars, setChars] = useState(0);
  const [leader, setLeader] = useState(false);
  const [clean, setClean] = useState(CFG.clean ?? false);
  const [diffs, setDiffs] = useState<{ path: string; added: number; removed: number; preview: { kind: string; text: string }[] }[]>([]);
  const [diffView, setDiffView] = useState<null | { path: string; added: number; removed: number; preview: { kind: string; text: string }[] }>(null);
  // T3 dialog kit: one fuzzy list-dialog primitive; the query is built from
  // raw keystrokes (onInput-per-keystroke destabilizes this OpenTUI build).
  const [dlg, setDlg] = useState<null | { title: string; items: DialogItem[]; onPick: (it: DialogItem) => void; onRename?: (id: string, title: string) => void }>(null);
  const [dlgQ, setDlgQ] = useState("");
  const [dlgSel, setDlgSel] = useState(0);
  const [dlgRename, setDlgRename] = useState<null | { id: string; text: string }>(null);
  const runId = useRef("");
  const streamRef = useRef("");
  const leaderTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const refreshSessions = () => listSessions(OPTS.path, 30).then((s) => Array.isArray(s) && setSessions(s)).catch(() => {});
  useEffect(() => { refreshSessions(); }, []);

  const push = (ls: Line[]) => {
    if (!ls.length) return;
    setLines((prev) => [...prev, ...ls]);
    setChars((c) => c + ls.reduce((n, l) => n + l.text.length, 0));
  };

  const collectDiff = (ev: AgentEvent) => {
    if (ev.type !== "file_edit") return;
    setDiffs((d) => [...d, {
      path: String(ev.path ?? "?"),
      added: Number(ev.added ?? 0),
      removed: Number(ev.removed ?? 0),
      preview: ((ev.preview as { kind: string; text: string }[]) ?? []),
    }]);
  };

  const openSession = (meta: SessionMeta) => {
    setActiveSid(meta.id);
    setActiveTitle(meta.title);
    setLines([{ kind: "sum", text: `— ${meta.title} · ${meta.backend} · ${meta.status} —` }]);
    setChars(0);
    setDiffs([]);
    sessionHistory(meta.id).then((evs) => {
      if (!Array.isArray(evs)) return;
      const all: Line[] = [];
      for (const ev of evs) { all.push(...eventLines(ev)); collectDiff(ev); }
      push(all);
    }).catch(() => {});
  };

  async function send(promptRaw: string) {
    let prompt = promptRaw.trim();
    if (!prompt || busy) return;
    // bare slash input → resolve through the registry (menu selection wins)
    if (prompt.startsWith("/") && !prompt.includes(" ")) {
      const q = prompt.slice(1).toLowerCase();
      const cmd = COMMANDS.find((c) => c.name.startsWith(q)) ?? COMMANDS.find((c) => fuzzy(q, c.name));
      if (cmd) { cmd.run(); return; }
      push([{ kind: "err", text: `unknown command ${prompt} — /help lists everything` }]);
      return;
    }
    // slash with args → arg-taking commands (except /conduct & /one-shot below)
    if (prompt.startsWith("/") && !prompt.startsWith("/conduct ") && !prompt.startsWith("/one-shot ")) {
      const [head, ...rest] = prompt.slice(1).split(" ");
      const cmd = COMMANDS.find((c) => c.args && c.name.startsWith(head.toLowerCase()));
      if (cmd) { cmd.run(rest.join(" ")); return; }
    }
    let isConduct = runMode === "conduct", apply = OPTS.apply || runMode === "apply" || runMode === "one-shot", yolo = OPTS.yolo || runMode === "one-shot";
    if (prompt.startsWith("/conduct ")) { isConduct = true; prompt = prompt.slice(9); }
    if (prompt.startsWith("/one-shot ")) { prompt = prompt.slice(10); apply = true; yolo = true; }
    push([{ kind: "user", text: `you · ${prompt}` }]);
    setBusy(true);
    setStatus(isConduct ? "conducting…" : runMode === "one-shot" ? "building…" : "thinking…");
    streamRef.current = "";
    try {
      const res: RunResult = await runSession(
        { path: OPTS.path, prompt, backend, model, apply, yolo, conduct: isConduct },
        (id) => (runId.current = id),
        (ev) => {
          if (ev.type === "message_delta") { streamRef.current += String(ev.text ?? ""); setStream(streamRef.current); return; }
          if (ev.type === "message") { const t = String(ev.text ?? streamRef.current); streamRef.current = ""; setStream(""); push([{ kind: "text", text: t }]); return; }
          if (ev.type === "done" && streamRef.current) { push([{ kind: "text", text: streamRef.current }]); streamRef.current = ""; setStream(""); }
          collectDiff(ev);
          push(eventLines(ev));
        },
      );
      push(res.error ? [{ kind: "err", text: String(res.error) }] : [{ kind: "sum", text: `done · ${res.changes} change(s)${res.applied ? " · applied" : ""}` }]);
      // a fresh conversation IS a session — adopt it so /rename /pin /export
      // work immediately (the run id is the session id)
      if (!activeSid && runId.current) {
        setActiveSid(runId.current);
        setActiveTitle(prompt.slice(0, 48));
      }
      refreshSessions();
    } catch (e) {
      push([{ kind: "err", text: `daemon: ${String(e)}` }]);
    }
    setBusy(false);
    setStatus("ready");
  }

  const openModelsDialog = () => {
    listModels().then((m) => {
      const all = [{ id: "", label: "Auto — best model per task", backend: "auto" }, ...m.claude, ...m.opencode];
      openDialog(
        "models",
        all.map((mm) => ({
          id: `${mm.backend} ${mm.id}`,
          label: mm.label,
          hint: mm.backend === "auto" ? undefined : mm.backend,
          live: (mm.id || undefined) === model && (mm.backend === "auto") === (backend === "auto"),
        })),
        (it) => {
          const [be, id] = it.id.split(" ");
          setBackend(be === "auto" ? "auto" : be);
          setModel(id || undefined);
          setModelLabel(it.label);
          saveTuiConfig({ backend: be, model: id || undefined, model_label: it.label });
        },
      );
    }).catch(() => {});
  };
  const openThemesDialog = () => {
    const items: DialogItem[] = [
      { id: "", label: "Lectern dark (built-in)", hint: "default" },
      ...listThemeFiles().map((t) => ({ id: t.file, label: t.name, hint: t.base })),
    ];
    openDialog("theme", items, (it) => {
      const ok = applyThemeFile(it.id || null);
      setThemeV((v) => v + 1);
      saveTuiConfig({ theme: it.id || null });
      if (!ok) push([{ kind: "err", text: `couldn't read theme ${it.label} — reverted to built-in` }]);
    });
  };
  const openBrain = () => {
    brainStats(OPTS.path).then((b) =>
      setPanel({ title: "Brain (read-only)", rows: [
        `sessions remembered  ${b.sessions}`,
        `learned skills       ${b.skills}`,
        `code graph           ${b.graph ? "indexed (graphify-out/)" : "not built — run graphify"}`,
        "",
        "The brain recalls files + skills into every run automatically.",
      ] })).catch(() => {});
  };
  const openUsage = () => {
    usageStats().then((u) => {
      const days = (u.days ?? []).slice(0, 7).reverse();
      const backends = u.backends ?? [];
      const fmtK = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n));
      setPanel({ title: "Usage (local store)", rows: [
        "last 7 active days — in / out tokens",
        ...days.map((d) => `  ${d.day}   ${fmtK(d.input)} / ${fmtK(d.output)}`),
        "",
        "by backend",
        ...backends.map((b) => `  ${b.backend.padEnd(12)} ${fmtK(b.input)} / ${fmtK(b.output)}`),
        "",
        "full charts live in the desktop app's Usage page",
      ] });
    }).catch((e) => push([{ kind: "err", text: String(e) }]));
  };
  const openMcpServers = () => {
    mcpOverview().then((m) => {
      const row = (name: string, list?: string[]) =>
        `  ${name.padEnd(12)} ${list && list.length ? list.join(", ").slice(0, 44) : "—"}`;
      setPanel({ title: "MCP servers (read-only)", rows: [
        "registered per harness (from their config files)",
        "",
        row("claude", m.claude),
        row("opencode", m.opencode),
        row("antigravity", m.antigravity),
        "",
        "add/remove servers in the desktop app → Settings → Tools (MCP)",
      ] });
    }).catch((e) => push([{ kind: "err", text: String(e) }]));
  };
  const openSkills = () => {
    listSkills(OPTS.path).then((sk) =>
      setPanel({ title: "Skills (read-only)", rows: sk.length
        ? sk.slice(0, 14).map((s) => `${s.name}  · used ${s.uses}×${s.triggers.length ? ` · ${s.triggers.slice(0, 2).join(", ")}` : ""}`)
        : ["No skills learned yet — /record one in the desktop app.", "", "Hub: github.com/ShrimpScript/lectern-hub"] })).catch(() => {});
  };

  // ── T2: ONE command registry drives slash commands, the ^X leader, and /help ──
  const newSession = () => {
    setActiveSid(null); setActiveTitle("new session"); setLines([]); setChars(0); setDiffs([]);
  };
  type Cmd = { name: string; key?: string; desc: string; args?: boolean; run: (args?: string) => void };
  const requireActive = (fn: (sid: string) => void) => {
    if (!activeSid) { push([{ kind: "err", text: "no active session — open one with /sessions first" }]); return; }
    fn(activeSid);
  };
  const doRename = (args?: string) => requireActive((sid) => {
    const title = (args ?? "").trim();
    if (!title) { push([{ kind: "err", text: "usage: /rename <new title>" }]); return; }
    renameSession(sid, title).then((r) => {
      if (r?.error) push([{ kind: "err", text: String(r.error) }]);
      else { setActiveTitle(title); refreshSessions(); push([{ kind: "sum", text: `renamed → ${title}` }]); }
    }).catch((e) => push([{ kind: "err", text: String(e) }]));
  });
  const doPin = () => requireActive((sid) => {
    const cur = sessions.find((s) => s.id === sid)?.pinned ?? false;
    pinSession(sid, !cur).then((r) => {
      if (r?.error) push([{ kind: "err", text: String(r.error) }]);
      else { refreshSessions(); push([{ kind: "sum", text: r.pinned ? "pinned ★" : "unpinned" }]); }
    }).catch((e) => push([{ kind: "err", text: String(e) }]));
  });
  const doExport = (args?: string) => requireActive((sid) => {
    const fmt = (args ?? "md").trim().toLowerCase() === "json" ? "json" : "md";
    sessionHistory(sid).then((evs) => {
      if (!Array.isArray(evs)) { push([{ kind: "err", text: "no history to export" }]); return; }
      const fs = require("node:fs") as typeof import("node:fs");
      const base = `lectern-${activeTitle.replace(/[^a-z0-9-]+/gi, "-").slice(0, 40)}-${sid.slice(0, 8)}`;
      const file = `${OPTS.path}/${base}.${fmt}`;
      if (fmt === "json") {
        fs.writeFileSync(file, JSON.stringify({ lectern_chat: 1, title: activeTitle, events: evs }, null, 2));
      } else {
        const lines = [`# ${activeTitle}`, ""];
        for (const ev of evs) for (const l of eventLines(ev)) lines.push(l.kind === "user" ? `\n**${l.text}**` : l.text);
        fs.writeFileSync(file, lines.join("\n") + "\n");
      }
      push([{ kind: "sum", text: `exported → ${file}` }]);
    }).catch((e) => push([{ kind: "err", text: String(e) }]));
  });
  const COMMANDS: Cmd[] = [
    { name: "sessions", key: "s", desc: "switch session (fuzzy)", run: () => openSessionsDialog() },
    { name: "models", key: "m", desc: "pick the model (fuzzy)", run: () => openModelsDialog() },
    { name: "new", key: "n", desc: "start a fresh session", run: () => newSession() },
    { name: "theme", key: "t", desc: "switch theme (reads ~/.lectern/themes)", run: () => openThemesDialog() },
    { name: "plan", desc: "plan mode — propose, don't apply (default)", run: () => setRunMode("plan") },
    { name: "apply", key: "a", desc: "apply mode — write changes, ask on risk", run: () => setRunMode("apply") },
    { name: "conduct", key: "c", desc: "conduct mode — orchestrate steps (or /conduct <task>)", run: () => setRunMode((m) => m === "conduct" ? "plan" : "conduct") },
    { name: "one-shot", key: "o", desc: "one-shot mode — build autonomously (or /one-shot <task>)", run: () => setRunMode((m) => m === "one-shot" ? "plan" : "one-shot") },
    { name: "rename", args: true, desc: "rename this session: /rename <title>", run: (a) => doRename(a) },
    { name: "pin", key: "p", desc: "pin/unpin this session (★ sorts first)", run: () => doPin() },
    { name: "export", args: true, desc: "export chat here: /export [md|json]", run: (a) => doExport(a) },
    { name: "brain", key: "b", desc: "brain stats", run: () => openBrain() },
    { name: "diffs", key: "d", desc: "file changes this session (full view)", run: () => {
      if (!diffs.length) { push([{ kind: "sum", text: "no file changes in this session yet" }]); return; }
      openDialog("diffs", diffs.map((d, i) => ({ id: String(i), label: d.path, hint: `+${d.added} −${d.removed}` })),
        (it) => setDiffView(diffs[Number(it.id)] ?? null));
    } },
    { name: "clean", key: "v", desc: "toggle clean output (hide machinery)", run: () => setClean((v) => { saveTuiConfig({ clean: !v }); return !v; }) },
    { name: "usage", key: "u", desc: "token usage from the local store", run: () => openUsage() },
    { name: "mcp-servers", desc: "MCP servers registered per harness", run: () => openMcpServers() },
    { name: "skills", key: "k", desc: "learned skills", run: () => openSkills() },
    { name: "help", key: "h", desc: "every command and key", run: () => setPanel({ title: "Help", rows: [
      "Type /command, or press ^X then a letter.",
      "",
      ...COMMANDS.map((c) => `/${c.name.padEnd(10)} ${c.key ? `^X ${c.key}` : "    "}  ${c.desc.length > 44 ? c.desc.slice(0, 43) + "…" : c.desc}`),
      "",
      "^P models · ^S sessions · ^B brain · ^K skills",
      "esc cancels a run / closes overlays · ^C quits",
      "",
      "need a shell? the TUI lives in your terminal — use your multiplexer",
      "(tmux/zellij) or ctrl+z; the embedded terminal is a desktop-app feature",
      "",
      `lectern-tui v${TUI_VERSION} · open source (Apache-2.0) · github.com/ShrimpScript/lectern`,
    ] }) },
    { name: "quit", key: "q", desc: "exit the TUI", run: () => process.exit(0) },
  ];
  const fuzzy = (needle: string, hay: string) => {
    let i = 0;
    for (const ch of hay) if (ch === needle[i]) i++;
    return i >= needle.length;
  };

  const openDialog = (
    title: string,
    items: DialogItem[],
    onPick: (it: DialogItem) => void,
    onRename?: (id: string, title: string) => void,
  ) => {
    setDlg({ title, items, onPick, onRename });
    setDlgQ("");
    setDlgSel(0);
    setDlgRename(null);
  };
  const openSessionsDialog = () => {
    refreshSessions();
    openDialog(
      "sessions",
      sessions.map((s) => ({
        id: s.id,
        label: (s.pinned ? "★ " : "") + s.title,
        hint: [s.backend, s.meta?.model && s.meta.model !== "auto" ? s.meta.model : "", s.meta?.project ?? ""].filter(Boolean).join(" · "),
        live: s.status === "running",
      })),
      (it) => { const m = sessions.find((s) => s.id === it.id); if (m) openSession(m); },
      (id, title) => {
        renameSession(id, title).then((r) => {
          if (r?.error) { push([{ kind: "err", text: String(r.error) }]); return; }
          if (id === activeSid) setActiveTitle(title);
          listSessions(OPTS.path, 30).then((ss) => {
            if (!Array.isArray(ss)) return;
            setSessions(ss);
            setDlg((d) => d && { ...d, items: ss.map((s) => ({ id: s.id, label: (s.pinned ? "★ " : "") + s.title, hint: s.backend, live: s.status === "running" })) });
          }).catch(() => {});
        }).catch((e) => push([{ kind: "err", text: String(e) }]));
      },
    );
  };
  const dlgFiltered = dlg ? dlg.items.filter((it) => fuzzy(dlgQ.toLowerCase(), it.label.toLowerCase())) : [];

  useKeyboard((key) => {
    if (diffView) {
      if (key.name === "escape" || key.name === "q") setDiffView(null);
      return;
    }
    if (dlg) {
      // inline rename sub-mode (^R on a row; sessions dialog only)
      if (dlgRename) {
        if (key.name === "escape") { setDlgRename(null); return; }
        if (key.name === "return" || key.name === "enter" || key.name === "linefeed") {
          const t = dlgRename.text.trim();
          if (t && dlg.onRename) dlg.onRename(dlgRename.id, t);
          setDlgRename(null);
          return;
        }
        if (key.name === "backspace" || key.name === "delete") { setDlgRename((r) => r && { ...r, text: r.text.slice(0, -1) }); return; }
        const seq = (key as { sequence?: string }).sequence ?? "";
        if (seq.length === 1 && !key.ctrl && !key.meta && seq >= " " && seq <= "~") {
          setDlgRename((r) => r && { ...r, text: r.text + seq });
        }
        return;
      }
      if (key.ctrl && key.name === "r" && dlg.onRename) {
        const it = dlgFiltered[Math.min(dlgSel, Math.max(0, dlgFiltered.length - 1))];
        if (it) setDlgRename({ id: it.id, text: it.label.replace(/^★ /, "") });
        return;
      }
      // modal: the dialog owns the keyboard; query from printable keys
      if (key.name === "escape") { setDlg(null); return; }
      if (key.name === "up") { setDlgSel((v) => Math.max(0, v - 1)); return; }
      if (key.name === "down") { setDlgSel((v) => Math.min(Math.max(0, dlgFiltered.length - 1), v + 1)); return; }
      if (key.name === "return" || key.name === "enter" || key.name === "linefeed") {
        const it = dlgFiltered[Math.min(dlgSel, dlgFiltered.length - 1)];
        if (it) { dlg.onPick(it); setDlg(null); }
        return;
      }
      if (key.name === "backspace" || key.name === "delete") { setDlgQ((q) => q.slice(0, -1)); setDlgSel(0); return; }
      const seq = (key as { sequence?: string }).sequence ?? "";
      if (seq.length === 1 && !key.ctrl && !key.meta && seq >= " " && seq <= "~") {
        setDlgQ((q) => q + seq);
        setDlgSel(0);
      }
      return;
    }
    if (leader) {
      setLeader(false);
      if (leaderTimer.current) clearTimeout(leaderTimer.current);
      const cmd = COMMANDS.find((c) => c.key === key.name);
      if (cmd) cmd.run();
      return;
    }
    if (key.ctrl && key.name === "x") {
      setLeader(true);
      if (leaderTimer.current) clearTimeout(leaderTimer.current);
      leaderTimer.current = setTimeout(() => setLeader(false), 2500);
      return;
    }
    if (panel && key.name === "escape") { setPanel(null); return; }
    if (key.ctrl && key.name === "p") { openModelsDialog(); return; }
    if (key.ctrl && key.name === "s") { openSessionsDialog(); return; }
    if (key.ctrl && key.name === "b") { openBrain(); return; }
    if (key.ctrl && key.name === "k") { openSkills(); return; }
    if (key.name === "escape" && busy && runId.current) { cancelRun(runId.current); setStatus("cancelling…"); }
  });

  const ctxPct = Math.min(100, Math.round((chars / 4 / 200_000) * 100));
  const mode = runMode;
  const modeColor = mode === "plan" ? C.fg3 : mode === "apply" ? C.accent : C.warn;
  const cwdShort = OPTS.path.split("/").filter(Boolean).slice(-1)[0] ?? OPTS.path;
  return (
    <box style={{ flexDirection: "column", width: "100%", height: "100%", backgroundColor: C.bg }}>
      <box style={{ height: 1, flexDirection: "row", paddingLeft: 1, paddingRight: 1, backgroundColor: C.panel }}>
        <text fg={C.fg}><b>⌊⌋ lectern</b></text>
        <text fg={C.fg3}>  ·  </text>
        <text fg={C.fg2}>{activeTitle}</text>
        <text fg={C.fg3}>  ·  {cwdShort}</text>
        <box style={{ flexGrow: 1 }} />
        <text fg={C.fg3}>^S sessions</text>
      </box>
      <scrollbox style={{ flexGrow: 1, border: true, borderColor: C.bd, padding: 1 }} stickyScroll stickyStart="bottom">
        {lines.length === 0 && !busy && (
          <text fg={C.fg3}>  Describe a task below — /conduct orchestrates, /one-shot builds autonomously. ^S sessions · ^P model · ^B brain · ^K skills</text>
        )}
        {(clean ? lines.filter((l) => l.kind === "user" || l.kind === "text" || l.kind === "err" || l.kind === "sum") : lines).map((l, i) => (
          <text key={i} fg={lineColor(l.kind)}>{l.kind === "user" ? "" : "  "}{l.text}</text>
        ))}
        {busy && stream && <text fg="#dededa">  {stream}▌</text>}
        {busy && !stream && <text fg={C.fg3}>  {status}</text>}
      </scrollbox>
      <box style={{ height: 3, border: true, borderColor: C.bd2, paddingLeft: 1, paddingRight: 1 }}
        title={busy ? " esc to cancel " : ` ${mode} mode `}>
        <input key={gen} focused={!panel && !dlg} placeholder="what should we build?  (/help for commands)"
          onSubmit={(v: string) => { setGen((g) => g + 1); send(v); }} />
      </box>
      <box style={{ height: 1, flexDirection: "row", paddingLeft: 1, paddingRight: 1, backgroundColor: C.panel }}>
        <text fg={C.fg2}>{modelLabel}</text>
        <text fg={C.fg3}>  ·  </text>
        <text fg={modeColor}>{mode}</text>
        <text fg={C.fg3}>  ·  ctx {ctxPct}%</text>
        {clean && <text fg={C.fg3}>  ·  clean</text>}
        <box style={{ flexGrow: 1 }} />
        {leader ? (
          <text fg={C.warn}>^X — s sessions · m models · n new · b brain · k skills · h help · q quit</text>
        ) : (
          <text fg={busy ? C.accent : C.fg3}>{status}</text>
        )}
      </box>
      {dlg && (
        <box style={{ position: "absolute", left: 6, top: 2, width: 56, border: true, borderColor: C.bd2, backgroundColor: C.panel, flexDirection: "column", padding: 1, zIndex: 10 }} title={` ${dlg.title} — filter · ↑↓ · enter${dlg.onRename ? " · ^R rename" : ""} · esc `}>
          {dlgRename
            ? <text fg={C.warn}>{`rename: ${dlgRename.text}▏`}</text>
            : <text fg={C.fg2}>{`▸ ${dlgQ}▏`}</text>}
          <text fg={C.fg3}> </text>
          {dlgFiltered.length === 0 && <text fg={C.fg3}>  nothing matches “{dlgQ}”</text>}
          {dlgFiltered.slice(0, 12).map((it, i) => (
            <text key={it.id} fg={i === dlgSel ? C.fg : C.fg2} bg={i === dlgSel ? C.panel2 : undefined}
              onMouseDown={() => { dlg.onPick(it); setDlg(null); }}>
              {(it.live ? "● " : "  ") + it.label.slice(0, 40) + (it.hint ? `  · ${it.hint}` : "")}
            </text>
          ))}
        </box>
      )}
      {diffView && (
        <box style={{ position: "absolute", left: 2, top: 1, right: 2, bottom: 2, border: true, borderColor: C.bd2, backgroundColor: C.bg, flexDirection: "column", padding: 1, zIndex: 20 }}
          title={` ✎ ${diffView.path}  +${diffView.added} −${diffView.removed} — esc `}>
          <scrollbox style={{ flexGrow: 1 }}>
            {diffView.preview.map((l, i) => (
              <text key={i} fg={l.kind === "add" ? "#9fe0ad" : l.kind === "remove" ? "#e58a97" : C.fg2}>
                {(l.kind === "add" ? "+ " : l.kind === "remove" ? "− " : "  ") + l.text}
              </text>
            ))}
            {diffView.preview.length === 0 && <text fg={C.fg3}>  (no preview lines recorded for this edit)</text>}
          </scrollbox>
        </box>
      )}
      {panel && (
        <box style={{ position: "absolute", left: 8, top: 1, width: 74, border: true, borderColor: C.bd2, backgroundColor: C.panel, flexDirection: "column", padding: 1, zIndex: 10 }} title={` ${panel.title} — esc `}>
          {panel.rows.map((r, i) => <text key={i} fg={r.includes("·") ? C.fg2 : C.fg3}>{r}</text>)}
        </box>
      )}
    </box>
  );
}

if (!(await ensureDaemon())) {
  console.error("lecternd unreachable — start it with `lecternd` (or set LECTERND_BIN)");
  process.exit(1);
}
const renderer = await createCliRenderer({ exitOnCtrlC: true });
createRoot(renderer).render(<App />);
