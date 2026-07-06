import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Scroll, Section } from "./App";
import { providerIcon } from "./BrandIcons";

/* Mission D5 — at-a-glance usage: totals, last-14-days bars, per-backend split,
   recent sessions. All from the local store's persisted usage events — no cloud. */
type UsageData = {
  days: { day: string; input: number; output: number }[];
  backends: { backend: string; input: number; output: number }[];
  recent: { title: string; backend: string; input: number; output: number; ts: number }[];
  total_input: number;
  total_output: number;
};

const fmt = (n: number) => (n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n));

export default function Usage() {
  const [d, setD] = useState<UsageData | null>(null);
  const [view, setView] = useState<"grid" | "bars">("grid");
  const [err, setErr] = useState(false);
  useEffect(() => { invoke<UsageData>("usage_stats").then(setD).catch(() => setErr(true)); }, []);
  const today = new Date().toISOString().slice(0, 10);
  const todayRow = d?.days.find((x) => x.day === today);
  const card = (label: string, value: string, sub: string) => (
    <div style={{ flex: 1, minWidth: 150, border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", padding: "15px 16px" }}>
      <div style={{ fontSize: 12, color: "var(--fg3)", fontWeight: 600 }}>{label}</div>
      <div style={{ fontSize: 26, fontWeight: 800, letterSpacing: "-0.02em", marginTop: 4 }}>{value}</div>
      <div style={{ fontSize: 11.5, color: "var(--fg3)", marginTop: 2 }}>{sub}</div>
    </div>
  );
  const maxDay = Math.max(1, ...(d?.days ?? []).map((x) => x.input + x.output));
  return (
    <Scroll>
      <div style={{ maxWidth: 760, margin: "0 auto", padding: "26px 24px 40px" }}>
        <div style={{ fontSize: 22, fontWeight: 800, letterSpacing: "-0.02em", marginBottom: 4 }}>Usage</div>
        <div style={{ fontSize: 13, color: "var(--fg2)", marginBottom: 20 }}>Tokens across every run — read from your local history, never the cloud.</div>
        {err && <div style={{ fontSize: 13, color: "var(--fg2)" }}>No usage data yet — run something first.</div>}
        {d && (
          <>
            <div style={{ display: "flex", gap: 10, flexWrap: "wrap", marginBottom: 18 }}>
              {card("Today", fmt((todayRow?.input ?? 0) + (todayRow?.output ?? 0)), `${fmt(todayRow?.input ?? 0)} in · ${fmt(todayRow?.output ?? 0)} out`)}
              {card("All time", fmt(d.total_input + d.total_output), `${fmt(d.total_input)} in · ${fmt(d.total_output)} out`)}
              {card("Sessions tracked", String(d.recent.length >= 10 ? "10+" : d.recent.length), "most recent shown below")}
            </div>
            <Section label="Activity">
              <div style={{ border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", padding: "14px 16px 12px" }}>
                <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: 10 }}>
                  <div role="radiogroup" aria-label="Activity view" style={{ display: "flex", gap: 3, background: "var(--panel2)", border: "1px solid var(--bd)", borderRadius: 8, padding: 2 }}>
                    {(["grid", "bars"] as const).map((v) => (
                      <button key={v} role="radio" aria-checked={view === v} onClick={() => setView(v)}
                        style={{ height: 24, padding: "0 11px", border: "none", borderRadius: 6, cursor: "pointer", fontFamily: "inherit", fontSize: 11.5, fontWeight: 600, textTransform: "capitalize", background: view === v ? "var(--btn)" : "transparent", color: view === v ? "var(--btnfg)" : "var(--fg2)", transition: "background .18s ease, color .18s ease" }}>
                        {v}
                      </button>
                    ))}
                  </div>
                </div>
                {d.days.length === 0 && <div style={{ fontSize: 12, color: "var(--fg3)" }}>No runs yet.</div>}
                {view === "grid" ? <ActivityGrid days={d.days} /> : (
                  <div style={{ display: "flex", alignItems: "flex-end", gap: 6, height: 104 }}>
                    {[...d.days].reverse().slice(-14).map((x) => (
                      <div key={x.day} title={`${x.day}: ${fmt(x.input)} in · ${fmt(x.output)} out`} style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", gap: 5, minWidth: 0 }}>
                        <div style={{ width: "70%", minHeight: 3, height: `${Math.max(3, ((x.input + x.output) / maxDay) * 78)}px`, borderRadius: 4, background: "var(--fg)", opacity: 0.85, transition: "height .4s cubic-bezier(.3,1.2,.4,1)" }} />
                        <div className="mono" style={{ fontSize: 8.5, color: "var(--fg3)" }}>{x.day.slice(5)}</div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </Section>
            <Section label="By provider">
              <div style={{ border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", overflow: "hidden" }}>
                {d.backends.map((b, i) => {
                  const total = Math.max(1, d.total_input + d.total_output);
                  const share = (b.input + b.output) / total;
                  return (
                    <div key={b.backend} style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 14px", borderTop: i ? "1px solid var(--bd2)" : "none" }}>
                      <span style={{ width: 26, height: 26, border: "1px solid var(--bd2)", borderRadius: 7, display: "inline-flex", alignItems: "center", justifyContent: "center", color: "var(--fg)", flexShrink: 0 }}>{providerIcon(b.backend, 14) ?? <span className="mono" style={{ fontSize: 10 }}>{b.backend.slice(0, 2)}</span>}</span>
                      <span style={{ width: 110, fontSize: 13, fontWeight: 600, flexShrink: 0, textTransform: "capitalize" }}>{b.backend.replace("-", " ")}</span>
                      <div style={{ flex: 1, height: 7, borderRadius: 999, background: "var(--panel2)", overflow: "hidden" }}>
                        <div style={{ width: `${Math.max(2, share * 100)}%`, height: "100%", borderRadius: 999, background: "var(--fg)", opacity: 0.8 }} />
                      </div>
                      <span className="mono" style={{ fontSize: 11, color: "var(--fg3)", flexShrink: 0 }}>{fmt(b.input)} in · {fmt(b.output)} out</span>
                    </div>
                  );
                })}
                {d.backends.length === 0 && <div style={{ padding: "12px 14px", fontSize: 12, color: "var(--fg3)" }}>No runs yet.</div>}
              </div>
            </Section>
            <Section label="Recent sessions">
              <div style={{ border: "1px solid var(--bd)", borderRadius: 12, background: "var(--panel)", overflow: "hidden" }}>
                {d.recent.map((r, i) => (
                  <div key={i} style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 14px", borderTop: i ? "1px solid var(--bd2)" : "none", fontSize: 12.5 }}>
                    <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontWeight: 600 }}>{r.title}</span>
                    <span className="mono" style={{ fontSize: 10.5, color: "var(--fg3)", flexShrink: 0 }}>{r.backend}</span>
                    <span className="mono" style={{ fontSize: 10.5, color: "var(--fg3)", flexShrink: 0 }}>{fmt(r.input)} in · {fmt(r.output)} out</span>
                  </div>
                ))}
                {d.recent.length === 0 && <div style={{ padding: "12px 14px", fontSize: 12, color: "var(--fg3)" }}>No sessions yet.</div>}
              </div>
            </Section>
          </>
        )}
      </div>
    </Scroll>
  );
}


/* GitHub-style activity tiles: 18 weeks × 7 days, intensity = tokens that day.
   Monochrome steps of the fg token (no green — house rules). */
function ActivityGrid({ days }: { days: { day: string; input: number; output: number }[] }) {
  const byDay = new Map(days.map((d) => [d.day, d.input + d.output]));
  const max = Math.max(1, ...days.map((d) => d.input + d.output));
  const today = new Date();
  const cols: { day: string; total: number }[][] = [];
  // build back from this week's Sunday, 18 columns
  const start = new Date(today);
  start.setDate(start.getDate() - start.getDay() - 7 * 17);
  for (let w = 0; w < 18; w++) {
    const col: { day: string; total: number }[] = [];
    for (let d = 0; d < 7; d++) {
      const dt = new Date(start);
      dt.setDate(start.getDate() + w * 7 + d);
      if (dt > today) break;
      const key = dt.toISOString().slice(0, 10);
      col.push({ day: key, total: byDay.get(key) ?? 0 });
    }
    cols.push(col);
  }
  const level = (t: number) => (t === 0 ? 0 : t < max * 0.25 ? 1 : t < max * 0.5 ? 2 : t < max * 0.75 ? 3 : 4);
  const alpha = [0.06, 0.25, 0.45, 0.7, 1];
  const months: (string | null)[] = cols.map((col, i) => {
    const first = col[0]?.day;
    if (!first) return null;
    const m = new Date(first).toLocaleString("en", { month: "short" });
    const prev = cols[i - 1]?.[0]?.day;
    return !prev || new Date(prev).getMonth() !== new Date(first).getMonth() ? m : null;
  });
  return (
    <div>
      <div style={{ display: "flex", gap: 3, marginBottom: 4, paddingLeft: 0 }}>
        {months.map((m, i) => (
          <div key={i} className="mono" style={{ width: 13, fontSize: 8, color: "var(--fg3)", overflow: "visible", whiteSpace: "nowrap" }}>{m ?? ""}</div>
        ))}
      </div>
      <div style={{ display: "flex", gap: 3 }}>
        {cols.map((col, i) => (
          <div key={i} style={{ display: "flex", flexDirection: "column", gap: 3 }}>
            {col.map((c) => (
              <div key={c.day} title={`${c.day}: ${c.total === 0 ? "no activity" : fmt(c.total) + " tokens"}`}
                style={{ width: 13, height: 13, borderRadius: 3, background: "var(--fg)", opacity: alpha[level(c.total)], border: "1px solid var(--bd2)", transition: "opacity .3s ease" }} />
            ))}
          </div>
        ))}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 5, marginTop: 8, justifyContent: "flex-end" }}>
        <span className="mono" style={{ fontSize: 9, color: "var(--fg3)" }}>less</span>
        {alpha.map((a, i) => <span key={i} style={{ width: 11, height: 11, borderRadius: 3, background: "var(--fg)", opacity: a, border: "1px solid var(--bd2)" }} />)}
        <span className="mono" style={{ fontSize: 9, color: "var(--fg3)" }}>more</span>
      </div>
    </div>
  );
}
