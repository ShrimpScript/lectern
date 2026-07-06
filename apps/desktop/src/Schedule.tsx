/* Schedule — the read-only overview of queued/scheduled runs. Extracted from
   App.tsx and lazy-loaded (never on the startup path). */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Scroll, Section, type ScheduleInfo } from "./App";

// ── Brain (memory + skills graph) ────────────────────────────────────────────
export function Schedule({ path }: { path: string }) {
  const [items, setItems] = useState<ScheduleInfo[] | null>(null);
  const load = () => { invoke<ScheduleInfo[]>("list_all_schedules").then(setItems).catch(() => setItems([])); };
  useEffect(load, [path]);
  const cancel = async (id: string) => { await invoke("cancel_schedule", { id }).catch(() => {}); load(); };
  const fmt = (ts: number) => new Date(ts * 1000).toLocaleString();
  return (
    <Scroll>
      <div style={{ maxWidth: 720, margin: "0 auto", padding: "44px 40px", display: "flex", flexDirection: "column", gap: 26 }}>
        <div>
          <div style={{ fontSize: 26, fontWeight: 800, letterSpacing: "-0.02em" }}>Schedule</div>
          <div style={{ fontSize: 14, color: "var(--fg2)", marginTop: 4 }}>Everything queued to run later, across every workspace. To schedule something, use the clock button in a chat’s composer — it queues that prompt in that session’s folder. The Lectern daemon (lecternd) runs them when due.</div>
        </div>
        {(
          <>
            <Section label="All scheduled runs">
              {(items ?? []).some((it) => it.status !== "pending" && it.status !== "running") && (
                <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: 8 }}>
                  <button
                    onClick={() => invoke<number>("clear_finished_schedules").then(() => load()).catch(() => {})}
                    style={{ height: 26, padding: "0 11px", borderRadius: 8, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg2)", fontSize: 11.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>
                    Clear finished
                  </button>
                </div>
              )}
              {items === null ? (
                <div className="mono" style={{ fontSize: 12, color: "var(--fg3)" }}>loading…</div>
              ) : items.length === 0 ? (
                <div className="mono" style={{ fontSize: 12.5, color: "var(--fg3)", border: "1px dashed var(--bd)", borderRadius: 10, padding: 18, textAlign: "center" }}>Nothing scheduled. Use the clock button in a chat to queue a prompt for later.</div>
              ) : (
                <div style={{ border: "1px solid var(--bd)", borderRadius: 12, overflow: "hidden", background: "var(--panel)" }}>
                  {items.map((s, i) => (
                    <div key={s.id} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 14px", borderTop: i ? "1px solid var(--bd2)" : "none" }}>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <div style={{ fontSize: 13.5, color: "var(--fg)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{s.prompt}</div>
                        <div className="mono" style={{ fontSize: 11, color: "var(--fg3)", marginTop: 2 }}>{fmt(s.run_at)} · {s.backend}{s.apply ? " · apply" : ""} · {s.status}</div>
                      </div>
                      {s.status === "pending" && <button onClick={() => cancel(s.id)} style={{ flexShrink: 0, height: 28, padding: "0 12px", border: "1px solid var(--bd)", borderRadius: 7, background: "transparent", color: "var(--fg2)", cursor: "pointer", fontSize: 12, fontFamily: "inherit" }}>Cancel</button>}
                    </div>
                  ))}
                </div>
              )}
            </Section>
          </>
        )}
      </div>
    </Scroll>
  );
}
