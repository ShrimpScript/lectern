// Built-in code editor (CodeMirror 6) — open a file from the Files tab, edit it with syntax
// highlighting, save it back, and leave inline review comments to hand to an agent.
import { useState, useEffect, useMemo, useRef } from "react";
import { X } from "lucide-react";
import CodeMirror, { type ReactCodeMirrorRef } from "@uiw/react-codemirror";
import { javascript } from "@codemirror/lang-javascript";
import { rust } from "@codemirror/lang-rust";
import { python } from "@codemirror/lang-python";
import { json } from "@codemirror/lang-json";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { markdown } from "@codemirror/lang-markdown";
import { EditorView, Decoration, type DecorationSet } from "@codemirror/view";
import { invoke } from "@tauri-apps/api/core";

export type LineComment = { line: number; text: string };

function langFor(name: string) {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["ts", "tsx", "js", "jsx", "mjs", "cjs"].includes(ext))
    return [javascript({ typescript: ext.startsWith("ts"), jsx: ext.endsWith("x") })];
  if (ext === "rs") return [rust()];
  if (ext === "py") return [python()];
  if (ext === "json") return [json()];
  if (ext === "css" || ext === "scss") return [css()];
  if (ext === "html" || ext === "htm") return [html()];
  if (ext === "md" || ext === "markdown") return [markdown()];
  return [];
}

const DANGER = "#d4756b";

export function CodeEditor({
  path,
  name,
  dark,
  onClose,
  onAddToPrompt,
}: {
  path: string;
  name: string;
  dark: boolean;
  onClose: () => void;
  onAddToPrompt: (text: string) => void;
}) {
  const [content, setContent] = useState<string | null>(null);
  const [err, setErr] = useState("");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [comments, setComments] = useState<LineComment[]>([]);
  const [curLine, setCurLine] = useState(1);
  const cmRef = useRef<ReactCodeMirrorRef>(null);

  useEffect(() => {
    setContent(null);
    setErr("");
    setComments([]);
    setDirty(false);
    invoke<string>("read_text_file", { path }).then(setContent).catch((e) => setErr(String(e)));
  }, [path]);

  const save = async () => {
    if (content == null) return;
    setSaving(true);
    try {
      await invoke("write_text_file", { path, content });
      setDirty(false);
    } catch (e) {
      setErr(String(e));
    }
    setSaving(false);
  };

  // Cmd/Ctrl+S saves.
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (dirty) save();
      }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  });

  // Highlight commented lines (rebuilt when comments change).
  const exts = useMemo(() => {
    const base = langFor(name);
    const deco = EditorView.decorations.of((view): DecorationSet => {
      const ranges = comments
        .filter((c) => c.line >= 1 && c.line <= view.state.doc.lines)
        .map((c) => {
          const line = view.state.doc.line(c.line);
          return Decoration.line({ attributes: { class: "cm-commented" } }).range(line.from);
        });
      return Decoration.set(ranges, true);
    });
    return [...base, deco];
  }, [name, comments]);

  const addComment = () => {
    const text = window.prompt(`Review comment on line ${curLine}:`);
    if (text && text.trim())
      setComments((c) => [...c, { line: curLine, text: text.trim() }].sort((a, b) => a.line - b.line));
  };

  const sendComments = () => {
    if (comments.length === 0) return;
    const body = comments.map((c) => `- line ${c.line}: ${c.text}`).join("\n");
    onAddToPrompt(`Address these review comments in ${name}:\n${body}`);
    onClose();
  };

  const btn = (extra: React.CSSProperties = {}): React.CSSProperties => ({
    height: 28,
    padding: "0 12px",
    borderRadius: 8,
    border: "1px solid var(--bd)",
    background: "transparent",
    color: "var(--fg2)",
    fontSize: 12.5,
    fontWeight: 600,
    cursor: "pointer",
    fontFamily: "inherit",
    ...extra,
  });

  return (
    <div style={{ position: "absolute", inset: 0, background: "var(--bg)", display: "flex", flexDirection: "column", zIndex: 40 }}>
      <div style={{ height: 46, flexShrink: 0, borderBottom: "1px solid var(--bd)", display: "flex", alignItems: "center", gap: 10, padding: "0 12px" }}>
        <span className="mono" style={{ fontSize: 13, color: "var(--fg)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {name}
          {dirty ? <span style={{ color: "var(--fg3)" }}> ●</span> : null}
        </span>
        <span style={{ fontSize: 11.5, color: "var(--fg3)", flexShrink: 0 }}>Ln {curLine}</span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 8, flexShrink: 0 }}>
          <button onClick={addComment} style={btn()} title="Leave a review comment on the current line">+ Comment</button>
          {comments.length > 0 && (
            <button onClick={sendComments} style={btn({ borderColor: "var(--accent, #7ec98f)", color: "var(--fg)" })}>Send {comments.length} to agent</button>
          )}
          <button onClick={save} disabled={!dirty || saving} style={btn({ background: dirty ? "var(--btn)" : "transparent", color: dirty ? "var(--btnfg)" : "var(--fg3)", borderColor: dirty ? "transparent" : "var(--bd)", cursor: dirty ? "pointer" : "default" })}>
            {saving ? "Saving…" : "Save"}
          </button>
          <button onClick={onClose} style={btn()}>Close</button>
        </span>
      </div>
      <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
        <div style={{ flex: 1, minWidth: 0, overflow: "auto" }}>
          {err ? (
            <div style={{ padding: 22, color: DANGER, fontSize: 13.5 }}>{err}</div>
          ) : content == null ? (
            <div style={{ padding: 22, color: "var(--fg3)", fontSize: 13.5 }}>Loading…</div>
          ) : (
            <CodeMirror
              ref={cmRef}
              value={content}
              extensions={exts}
              theme={dark ? "dark" : "light"}
              height="100%"
              style={{ height: "100%", fontSize: 13 }}
              onChange={(v) => {
                setContent(v);
                setDirty(true);
              }}
              onUpdate={(vu) => {
                const ln = vu.state.doc.lineAt(vu.state.selection.main.head).number;
                setCurLine(ln);
              }}
            />
          )}
        </div>
        {comments.length > 0 && (
          <div style={{ width: 232, flexShrink: 0, borderLeft: "1px solid var(--bd)", overflow: "auto", padding: 12, display: "flex", flexDirection: "column", gap: 8, background: "var(--panel)" }}>
            <div style={{ fontSize: 11, fontWeight: 700, color: "var(--fg3)", textTransform: "uppercase", letterSpacing: ".04em" }}>Comments</div>
            {comments.map((c, i) => (
              <div key={i} style={{ border: "1px solid var(--bd)", borderRadius: 8, padding: "8px 10px", fontSize: 12, color: "var(--fg2)", background: "var(--bg)" }}>
                <div style={{ display: "flex", justifyContent: "space-between", gap: 6, marginBottom: 3 }}>
                  <span className="mono" style={{ color: "var(--fg3)", fontSize: 11 }}>line {c.line}</span>
                  <button onClick={() => setComments((cs) => cs.filter((_, j) => j !== i))} style={{ border: "none", background: "transparent", color: "var(--fg3)", cursor: "pointer", lineHeight: 1, padding: 0, display: "inline-flex" }}><X size={12} strokeWidth={1.8} /></button>
                </div>
                {c.text}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
