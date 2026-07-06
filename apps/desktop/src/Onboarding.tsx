import { useEffect, useRef, useState } from "react";
import { AntigravityIcon, ClaudeIcon } from "./BrandIcons";
import { RiveScene } from "./RiveScene";
import type { BackendInfo } from "./App";

/* Onboarding v2 — in-depth for people who've never touched an AI agent:
   ① what Lectern IS (plain language + a living demo scene) → ② connect the
   AI you already pay for → ③ pick a project (the brain explained) → ④ the
   modes. Scenes are Rive-ready (drop .riv files in assets/rive/) with
   code-driven cursor animations as the always-working fallback. */

const OpenCodeGlyph = (
  <svg width={16} height={16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
    <rect x="2.5" y="3.5" width="19" height="17" rx="2.5" />
    <path d="m7.5 9 3 3-3 3M12.5 15.5h4" />
  </svg>
);

/* A mini app mock driven by a JS phase machine: the cursor MEASURES
   its targets (getBoundingClientRect) and glides to them via CSS transitions, the
   input starts blank and types char-by-char, clicks pulse the real element. No
   hardcoded coordinates. Reduced motion → static final frame, cursor hidden. */
const TOUR_TEXT = "fix the login bug and add a test";
function DemoScene({ variant }: { variant: "tour" | "modes" }) {
  const stage = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLSpanElement | null>(null);
  const orbRef = useRef<HTMLSpanElement | null>(null);
  const chipRef = useRef<HTMLSpanElement | null>(null);
  const [phase, setPhase] = useState(0);
  const [typed, setTyped] = useState("");
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null);
  const [pressed, setPressed] = useState(false);
  const reduced = typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

  // Cursor tip → the CENTER of a measured element (stage-relative).
  const moveTo = (el: Element | null, dx = 0, dy = 0) => {
    if (!el || !stage.current) return;
    const s = stage.current.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    setCursor({ x: r.left - s.left + r.width / 2 + dx, y: r.top - s.top + r.height / 2 + dy });
  };

  useEffect(() => {
    if (reduced) { setTyped(TOUR_TEXT); setPhase(9); return; }
    let alive = true;
    const timers: ReturnType<typeof setTimeout>[] = [];
    const at = (ms: number, fn: () => void) => timers.push(setTimeout(() => alive && fn(), ms));
    const loop = () => {
      if (!alive) return;
      setPhase(0); setTyped(""); setPressed(false);
      if (variant === "tour") {
        at(200, () => { setCursor({ x: 250, y: 30 }); });
        at(600, () => { setPhase(1); moveTo(inputRef.current, -40, 0); });
        at(1500, () => {
          setPhase(2);
          TOUR_TEXT.split("").forEach((_, i) =>
            at(1500 + i * 55 - 1500 + 0, () => {}));
          let i = 0;
          const t = setInterval(() => {
            if (!alive) { clearInterval(t); return; }
            i += 1; setTyped(TOUR_TEXT.slice(0, i));
            if (i >= TOUR_TEXT.length) clearInterval(t);
          }, 55);
          timers.push(t as unknown as ReturnType<typeof setTimeout>);
        });
        at(3600, () => { setPhase(3); moveTo(orbRef.current); });
        at(4400, () => { setPressed(true); });
        at(4600, () => { setPressed(false); setPhase(4); });
        at(5400, () => setPhase(5));
        at(8200, () => loop());
      } else {
        at(200, () => setCursor({ x: 280, y: 120 }));
        at(600, () => { setPhase(1); moveTo(chipRef.current); });
        at(1500, () => setPressed(true));
        at(1700, () => { setPressed(false); setPhase(2); });
        at(2500, () => setPhase(3));
        at(3300, () => setPhase(4));
        at(6800, () => loop());
      }
    };
    loop();
    return () => { alive = false; timers.forEach(clearTimeout); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [variant, reduced]);

  const show = (from: number) => ({ opacity: phase >= from ? 1 : 0, transform: phase >= from ? "none" : "translateY(4px)", transition: "opacity .3s ease, transform .3s ease" });
  return (
    <div ref={stage} className="onb-stage" aria-hidden>
      {variant === "tour" ? (
        <>
          <div className="onb-bubble" style={show(4)}>{TOUR_TEXT}</div>
          <div className="onb-card onb-plan" style={show(5)}>
            <div className="onb-plan-title">Plan</div>
            <div className="onb-plan-row">✓ Find the failing redirect</div>
            <div className="onb-plan-row">✓ Patch session refresh</div>
            <div className="onb-plan-row onb-plan-late">✓ Add regression test</div>
          </div>
          <div className="onb-composer">
            <span ref={inputRef} className="onb-type2">
              {typed || (phase < 2 ? <span style={{ color: "var(--fg3)" }}>what should we build?</span> : "")}
              {phase === 2 && typed.length < TOUR_TEXT.length && <span className="onb-caret" />}
            </span>
            <span ref={orbRef} className="onb-orb" style={{ transform: pressed ? "scale(.84)" : "scale(1)", transition: "transform .16s ease" }}>↑</span>
          </div>
        </>
      ) : (
        <>
          <div className="onb-chiprow">
            <span ref={chipRef} className="onb-chip" style={{ background: phase >= 2 ? "var(--hov)" : "var(--panel2)", color: phase >= 2 ? "var(--fg)" : "var(--fg2)", transform: pressed ? "scale(.92)" : "scale(1)", transition: "all .18s ease" }}>/conduct</span>
            <span className="onb-chip">/one-shot</span>
            <span className="onb-chip">/record</span>
          </div>
          <div className="onb-pill" style={show(2)}>conduct mode <span style={{ opacity: 0.55 }}>on</span></div>
          <div className="onb-route" style={show(3)}>Routed to Opus 4.8 · step 1/3</div>
          <div className="onb-route onb-route2" style={show(4)}>Routed to Gemini Flash · step 2/3</div>
        </>
      )}
      {!reduced && cursor && (
        <svg className="onb-cursor2" width="18" height="18" viewBox="0 0 24 24" fill="var(--fg)" stroke="var(--bg)" strokeWidth="1.4"
          style={{ left: 0, top: 0, transform: `translate(${cursor.x}px, ${cursor.y}px)` }}>
          <path d="M5 3l14 8-6.5 1.5L9 19z" />
        </svg>
      )}
    </div>
  );
}

const STEPS = ["What is Lectern", "Connect your AI", "Pick a project", "The modes"] as const;

export function Onboarding({ backends, hasFolder, onPickFolder, onRecheck, onDone }: {
  backends: BackendInfo[]; hasFolder: boolean; onPickFolder: () => void; onRecheck: () => void; onDone: () => void;
}) {
  const [step, setStep] = useState(0);
  const claude = backends.find((b) => b.id === "claude-code")?.available ?? false;
  const agy = backends.find((b) => b.id === "antigravity")?.available ?? false;
  const oc = backends.find((b) => b.id === "opencode")?.available ?? false;

  const row = (icon: React.ReactNode, ok: boolean, title: string, desc: string, action?: React.ReactNode) => (
    <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 14px", border: "1px solid var(--bd)", borderRadius: 11, background: "var(--panel)" }}>
      <span style={{ width: 30, height: 30, flexShrink: 0, border: "1px solid var(--bd2)", borderRadius: 8, display: "inline-flex", alignItems: "center", justifyContent: "center", color: "var(--fg)" }}>{icon}</span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontWeight: 600, fontSize: 13.5, display: "flex", alignItems: "center", gap: 7 }}>
          {title}
          {ok && <span style={{ fontSize: 10.5, color: "var(--fg3)", border: "1px solid var(--bd)", borderRadius: 5, padding: "1px 6px" }}>connected</span>}
        </div>
        <div style={{ fontSize: 12, color: "var(--fg2)", marginTop: 1, lineHeight: 1.45 }}>{desc}</div>
      </div>
      {action}
    </div>
  );
  const ghost = (label: string, onClick: () => void) => (
    <button onClick={onClick} style={{ flexShrink: 0, height: 28, padding: "0 12px", borderRadius: 8, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg)", fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>{label}</button>
  );

  const body = [
    // ① What is Lectern
    <div key="0">
      <RiveScene name="what-is-lectern.riv"><DemoScene variant="tour" /></RiveScene>
      <div style={{ fontSize: 13, color: "var(--fg2)", lineHeight: 1.6, marginTop: 14 }}>
        <b style={{ color: "var(--fg)" }}>Lectern is a cockpit for AI that writes code.</b> You describe what you
        want in plain words — it plans the work, edits the files, runs the commands, and shows you every step.
        <br /><br />
        It doesn&apos;t replace your AI subscription: it <i>drives</i> the AI you already have (Claude, Gemini, and
        more), and adds what they forget — a memory of your projects, learned skills, and the ability to hand
        each task to whichever model is best at it.
      </div>
    </div>,
    // ② Connect
    <div key="1" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ fontSize: 12.5, color: "var(--fg2)", lineHeight: 1.55, marginBottom: 4 }}>
        Lectern works through agent tools installed on your computer. Connect at least one — each unlocks its
        models in Lectern. Your logins and keys stay on this machine.
      </div>
      {row(<ClaudeIcon size={15} />, claude, "Claude Code", claude ? "Claude models are ready." : "Terminal: npm i -g @anthropic-ai/claude-code — then run claude once to log in.", claude ? undefined : ghost("Re-check", onRecheck))}
      {row(<AntigravityIcon size={15} />, agy, "Antigravity", agy ? "Gemini models are ready." : "Optional — install Antigravity, run agy once to sign in.", agy ? undefined : ghost("Re-check", onRecheck))}
      {row(OpenCodeGlyph, oc, "OpenCode", oc ? "OpenRouter + free models are ready." : "Optional — opencode.ai; its free models need no key at all.", oc ? undefined : ghost("Re-check", onRecheck))}
    </div>,
    // ③ Project
    <div key="2" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      {row(
        <svg width={15} height={15} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" /></svg>,
        hasFolder,
        "Open a project folder",
        hasFolder ? "A folder is set — you're ready." : "Pick any code folder (or an empty one to start something new).",
        hasFolder ? undefined : ghost("Choose…", onPickFolder),
      )}
      <div style={{ fontSize: 12.5, color: "var(--fg2)", lineHeight: 1.6, border: "1px solid var(--bd)", borderRadius: 11, padding: "12px 14px", background: "var(--panel)" }}>
        The first time you open a project, Lectern quietly reads it into its <b style={{ color: "var(--fg)" }}>brain</b> —
        so from then on, every session starts already knowing your files and conventions instead of exploring
        from scratch. Everything stays on your machine.
      </div>
    </div>,
    // ④ Modes
    <div key="3">
      <RiveScene name="modes.riv"><DemoScene variant="modes" /></RiveScene>
      <div style={{ display: "flex", flexDirection: "column", gap: 7, marginTop: 14, fontSize: 12.5, color: "var(--fg2)", lineHeight: 1.55 }}>
        <div><b style={{ color: "var(--fg)" }}>Just type</b> — describe a task; Lectern plans it and shows the changes for review.</div>
        <div><b style={{ color: "var(--fg)" }}>/conduct</b> — orchestrates: splits the task and hands each piece to the best model.</div>
        <div><b style={{ color: "var(--fg)" }}>/one-shot</b> — autonomous: give a short brief, it builds the whole thing.</div>
      </div>
    </div>,
  ][step];

  const last = step === STEPS.length - 1;
  return (
    <div style={{ position: "fixed", inset: 0, zIndex: 100, background: "var(--backdrop)", display: "flex", alignItems: "center", justifyContent: "center", padding: 24 }}>
      <div className="lectern-msg" style={{ width: "100%", maxWidth: 500, background: "var(--bg)", border: "1px solid var(--bd)", borderRadius: 18, padding: "26px 26px 20px", boxShadow: "0 30px 80px -20px rgba(0,0,0,.4)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 16 }}>
          <Logo22 />
          <div style={{ fontWeight: 800, fontSize: 15, letterSpacing: "-0.01em" }}>{STEPS[step]}</div>
          <div style={{ marginLeft: "auto", display: "flex", gap: 5 }}>
            {STEPS.map((_, i) => (
              <span key={i} onClick={() => setStep(i)} style={{ width: i === step ? 16 : 6, height: 6, borderRadius: 999, background: i === step ? "var(--fg)" : "var(--bd)", cursor: "pointer", transition: "all .25s cubic-bezier(.3,1.2,.4,1)" }} />
            ))}
          </div>
        </div>
        <div className="lectern-fadein" key={step}>{body}</div>
        <div style={{ display: "flex", gap: 8, marginTop: 18 }}>
          {step > 0 && (
            <button onClick={() => setStep(step - 1)} style={{ height: 38, padding: "0 16px", borderRadius: 10, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg2)", fontSize: 13, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>Back</button>
          )}
          <button onClick={() => (last ? onDone() : setStep(step + 1))} style={{ flex: 1, height: 38, borderRadius: 10, border: "none", background: "var(--btn)", color: "var(--btnfg)", fontSize: 13.5, fontWeight: 700, cursor: "pointer", fontFamily: "inherit" }}>
            {last ? "Start building" : "Next"}
          </button>
        </div>
        <button onClick={onDone} style={{ display: "block", margin: "9px auto 0", border: "none", background: "transparent", color: "var(--fg3)", fontSize: 11.5, cursor: "pointer", fontFamily: "inherit" }}>Skip the tour</button>
      </div>
    </div>
  );
}

function Logo22() {
  return (
    <div style={{ width: 22, height: 22, border: "1.5px solid var(--fg)", borderRadius: 4, display: "flex", alignItems: "center", justifyContent: "center", position: "relative", flexShrink: 0 }}>
      <div style={{ width: 2, height: 11, background: "var(--fg)" }} />
      <div style={{ position: "absolute", bottom: 3, width: 11, height: 2, background: "var(--fg)" }} />
    </div>
  );
}
