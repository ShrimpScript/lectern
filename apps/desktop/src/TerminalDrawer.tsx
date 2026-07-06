import { useEffect, useRef, useState } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

/* Embedded terminal (Hermes-inspired) — v2 after driving the real app:
   output rides a tauri Channel (the codebase's proven streaming mechanism;
   the emit/listen bridge never delivered in release), and the drawer opens
   as a blended inset panel — app chrome, radius, slide-up entrance — instead
   of a hard black slab. Terminal surfaces stay dark in both themes (house
   precedent: terminals read dark). */
type TermMsg = { kind: "out"; data: string } | { kind: "exit" };
type TermEngine = { id: string; label: string };

export default function TerminalDrawer({ sessionId, cwd, visible, onExit }: {
  sessionId: string;
  cwd: string;
  visible: boolean;
  onExit: () => void;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  // Backend picker (docker/ssh) — only rendered when something beyond
  // "local" is DETECTED, so most machines see no new chrome at all.
  const [engines, setEngines] = useState<TermEngine[]>([]);
  const [engine, setEngine] = useState("local");
  const [pickOpen, setPickOpen] = useState(false);
  useEffect(() => { invoke<TermEngine[]>("term_engines").then(setEngines).catch(() => {}); }, []);

  useEffect(() => {
    const el = hostRef.current;
    if (!el) return;
    const term = new Terminal({
      fontFamily: "'IBM Plex Mono', ui-monospace, monospace",
      fontSize: 12.5,
      lineHeight: 1.35,
      cursorBlink: true,
      theme: {
        background: "#101013",
        foreground: "#cfcfca",
        cursor: "#f4f4f2",
        selectionBackground: "#33333866",
        black: "#1d1d20", brightBlack: "#77776f",
        red: "#e5687a", brightRed: "#e58a97",
        green: "#7fd4a0", brightGreen: "#9fe0ad",
        yellow: "#e0b34d", brightYellow: "#e8c77d",
        blue: "#7aa7d9", brightBlue: "#9cbfe8",
        magenta: "#b48ead", brightMagenta: "#c9a9c4",
        cyan: "#88c0d0", brightCyan: "#a3d3e0",
        white: "#dededa", brightWhite: "#f4f4f2",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;

    const ch = new Channel<TermMsg>();
    ch.onmessage = (m) => {
      if (m.kind === "out") term.write(m.data);
      else {
        term.write("\r\n\x1b[2m— shell exited —\x1b[0m\r\n");
        onExit();
      }
    };
    invoke("term_open", { id: sessionId, cwd, cols: term.cols, rows: term.rows, engine, onOut: ch }).catch((err) => {
      term.write(`\x1b[31m${String(err)}\x1b[0m\r\n`);
    });
    const dataSub = term.onData((d) => invoke("term_write", { id: sessionId, data: d }).catch(() => {}));

    const ro = new ResizeObserver(() => {
      fit.fit();
      invoke("term_resize", { id: sessionId, cols: term.cols, rows: term.rows }).catch(() => {});
    });
    ro.observe(el);

    return () => {
      ro.disconnect();
      dataSub.dispose();
      invoke("term_kill", { id: sessionId }).catch(() => {});
      term.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, engine]);

  useEffect(() => {
    if (visible) {
      setTimeout(() => {
        fitRef.current?.fit();
        const t = termRef.current;
        if (t) invoke("term_resize", { id: sessionId, cols: t.cols, rows: t.rows }).catch(() => {});
        termRef.current?.focus();
      }, 240); // after the slide-in settles
    }
  }, [visible, sessionId]);

  const folder = cwd.split("/").filter(Boolean).pop() ?? "~";
  return (
    <div className={visible ? "lectern-termwrap open" : "lectern-termwrap"}>
      <div style={{ margin: "0 14px 10px", border: "1px solid var(--bd)", borderRadius: 12, overflow: "hidden", background: "#101013", boxShadow: "0 14px 40px -18px rgba(0,0,0,.5)", display: "flex", flexDirection: "column", height: 264 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "7px 8px 7px 12px", background: "var(--panel)", borderBottom: "1px solid var(--bd)", flexShrink: 0 }}>
          <span style={{ width: 22, height: 22, border: "1px solid var(--bd2)", borderRadius: 6, display: "inline-flex", alignItems: "center", justifyContent: "center", color: "var(--fg2)", flexShrink: 0 }}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden><rect x="3" y="4.5" width="18" height="15" rx="2.5" /><path d="m7.5 9.5 3 3-3 3M12.5 15.5h4.5" /></svg>
          </span>
          <span style={{ fontSize: 12.5, fontWeight: 600, color: "var(--fg)", flexShrink: 0 }}>{folder}</span>
          <span className="mono" style={{ fontSize: 10, color: "var(--fg3)", flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{cwd}</span>
          {engines.length > 1 && (
            <div style={{ position: "relative", flexShrink: 0 }}>
              <button onClick={() => setPickOpen((v) => !v)} data-menu-keep
                style={{ height: 24, padding: "0 10px", borderRadius: 7, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg2)", fontSize: 11, fontWeight: 600, cursor: "pointer", fontFamily: "inherit", display: "inline-flex", alignItems: "center", gap: 5 }}>
                {engines.find((e) => e.id === engine)?.label ?? "This computer"}
                <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" aria-hidden><path d="m6 9 6 6 6-6" /></svg>
              </button>
              {pickOpen && (
                <div className="lectern-pop" data-menu-keep style={{ position: "absolute", right: 0, top: 28, minWidth: 190, background: "var(--elev)", border: "1px solid var(--bd)", borderRadius: 10, padding: 5, zIndex: 40, boxShadow: "0 12px 32px -12px rgba(0,0,0,.45)" }}>
                  {engines.map((e) => (
                    <button key={e.id}
                      onClick={() => {
                        setPickOpen(false);
                        if (e.id === engine) return;
                        // switching kills the live shell — that's user state, so it's explicit
                        if (!window.confirm(`Switch terminal to ${e.label}? The current shell will close.`)) return;
                        invoke("term_kill", { id: sessionId }).catch(() => {});
                        setEngine(e.id);
                      }}
                      style={{ display: "block", width: "100%", textAlign: "left", padding: "7px 10px", borderRadius: 7, border: "none", background: e.id === engine ? "var(--hov)" : "transparent", color: "var(--fg)", fontSize: 12, cursor: "pointer", fontFamily: "inherit" }}>
                      {e.label}
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}
          <button onClick={() => termRef.current?.clear()}
            style={{ height: 24, padding: "0 10px", borderRadius: 7, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg2)", fontSize: 11, fontWeight: 600, cursor: "pointer", fontFamily: "inherit", flexShrink: 0 }}>Clear</button>
          <button onClick={onExit} title="Close terminal"
            style={{ width: 24, height: 24, display: "inline-flex", alignItems: "center", justifyContent: "center", borderRadius: 7, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg2)", cursor: "pointer", flexShrink: 0 }}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden><path d="m6 6 12 12M18 6 6 18" /></svg>
          </button>
        </div>
        <div ref={hostRef} style={{ flex: 1, minHeight: 0, padding: "6px 4px 8px 12px" }} />
      </div>
    </div>
  );
}
