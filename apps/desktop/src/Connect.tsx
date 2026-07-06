import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AirtableIcon, AsanaIcon, AtlassianIcon, ChromeIcon, CubeIcon, DiscordIcon, DocsIcon,
  DuckDuckGoIcon, FlameIcon, FlaskLineIcon, FolderIcon, GithubIcon, GraphMemIcon, IMessageIcon,
  LinearIcon, MongoIcon, NotionIcon, PayPalIcon, PlaywrightIcon, PostgresIcon, SearchLineIcon,
  SentryIcon, ShopifyIcon, StepsIcon, StripeIcon, SupabaseIcon, TelegramIcon, VercelIcon,
} from "./BrandIcons";
import { ACCENT, ctrl, Scroll, type McpServer } from "./App";

/* One-click MCP defaults — official servers with real brand marks. Rows with
   requirements expand an inline form (tokens/connection strings) and register
   via `claude mcp add -e KEY=VAL -- cmd…`. */
export type CatalogEntry = {
  key: string;
  name: string;
  desc: string;
  icon: React.ReactNode;
  command: string;
  env?: { key: string; label: string; placeholder: string }[];
  arg?: { label: string; placeholder: string };
  note: string;
  category: string;
};
export const MCP_CATALOG: CatalogEntry[] = [
  {
    key: "github",
    category: "Dev & code",
    name: "GitHub",
    desc: "Issues, PRs, repos — agents work your GitHub.",
    icon: <GithubIcon size={16} />,
    command: "npx -y @modelcontextprotocol/server-github",
    env: [{ key: "GITHUB_PERSONAL_ACCESS_TOKEN", label: "Personal access token", placeholder: "ghp_…" }],
    note: "needs a token",
  },
  {
    key: "playwright",
    category: "Dev & code",
    name: "Playwright browser",
    desc: "Real browser automation — navigate, click, screenshot.",
    icon: <PlaywrightIcon size={16} />,
    command: "npx -y @playwright/mcp@latest",
    note: "no keys needed",
  },
  {
    key: "postgres",
    category: "Data & infra",
    name: "Postgres",
    desc: "Let agents query your database.",
    icon: <PostgresIcon size={16} />,
    command: "npx -y @modelcontextprotocol/server-postgres",
    arg: { label: "Connection string", placeholder: "postgresql://user:pass@localhost:5432/db" },
    note: "needs a connection string",
  },
  {
    key: "filesystem",
    category: "Dev & code",
    name: "Filesystem",
    desc: "Read/write a folder you choose (scoped access).",
    icon: <FolderIcon size={16} />,
    command: "npx -y @modelcontextprotocol/server-filesystem",
    arg: { label: "Folder to allow", placeholder: "/home/you/projects" },
    note: "pick a folder",
  },
  {
    key: "memory",
    category: "Utilities",
    name: "Memory (graph)",
    desc: "Simple knowledge-graph memory. Note: Lectern's brain already does this and more.",
    icon: <GraphMemIcon size={16} />,
    command: "npx -y @modelcontextprotocol/server-memory",
    note: "no keys needed",
  },
  {
    key: "sequential-thinking",
    category: "Utilities",
    name: "Sequential thinking",
    desc: "Structured step-by-step reasoning tool for harder problems.",
    icon: <StepsIcon size={16} />,
    command: "npx -y @modelcontextprotocol/server-sequential-thinking",
    note: "no keys needed",
  },
  {
    key: "notion",
    category: "Productivity",
    name: "Notion",
    desc: "Pages and databases — agents read and update your workspace.",
    icon: <NotionIcon size={16} />,
    command: "npx -y @notionhq/notion-mcp-server",
    env: [{ key: "NOTION_TOKEN", label: "Notion integration token", placeholder: "ntn_…" }],
    note: "needs a token",
  },
  {
    key: "linear",
    category: "Productivity",
    name: "Linear",
    desc: "Issues and projects — official remote server.",
    icon: <LinearIcon size={16} />,
    command: "https://mcp.linear.app/mcp",
    note: "sign in with /mcp inside Claude Code after adding",
  },
  {
    key: "sentry",
    category: "Dev & code",
    name: "Sentry",
    desc: "Errors and traces — agents debug from real events.",
    icon: <SentryIcon size={16} />,
    command: "npx -y @sentry/mcp-server",
    env: [{ key: "SENTRY_AUTH_TOKEN", label: "Sentry auth token", placeholder: "sntrys_…" }],
    note: "needs a token",
  },
  {
    key: "stripe",
    category: "Business",
    name: "Stripe",
    desc: "Payments data — official Stripe agent toolkit.",
    icon: <StripeIcon size={16} />,
    command: "npx -y @stripe/mcp --tools=all",
    env: [{ key: "STRIPE_SECRET_KEY", label: "Stripe secret key (test mode recommended)", placeholder: "sk_test_…" }],
    note: "needs an API key",
  },
  {
    key: "context7",
    category: "Dev & code",
    name: "Context7",
    desc: "Up-to-date library docs injected into coding tasks.",
    icon: <DocsIcon size={16} />,
    command: "npx -y @upstash/context7-mcp",
    note: "no keys needed",
  },
  {
    key: "firecrawl",
    category: "Web & search",
    name: "Firecrawl",
    desc: "Scrape and crawl websites into clean agent-ready text.",
    icon: <FlameIcon size={16} />,
    command: "npx -y firecrawl-mcp",
    env: [{ key: "FIRECRAWL_API_KEY", label: "Firecrawl API key", placeholder: "fc-…" }],
    note: "needs an API key",
  },
  {
    key: "chrome-devtools",
    category: "Dev & code",
    name: "Chrome DevTools",
    desc: "Official Chrome MCP — inspect pages, console, network, performance.",
    icon: <ChromeIcon size={16} />,
    command: "npx -y chrome-devtools-mcp",
    note: "no keys needed",
  },
  {
    key: "shopify-dev",
    category: "Dev & code",
    name: "Shopify Dev",
    desc: "Official Shopify dev assistant — APIs, schemas, docs for building on Shopify.",
    icon: <ShopifyIcon size={16} />,
    command: "npx -y @shopify/dev-mcp",
    note: "no keys needed",
  },
  {
    key: "e2b",
    category: "Dev & code",
    name: "E2B sandboxes",
    desc: "Run agent-generated code in cloud sandboxes.",
    icon: <CubeIcon size={16} />,
    command: "npx -y @e2b/mcp-server",
    env: [{ key: "E2B_API_KEY", label: "E2B API key", placeholder: "e2b_…" }],
    note: "needs an API key",
  },
  {
    key: "supabase",
    category: "Data & infra",
    name: "Supabase",
    desc: "Query and manage your Supabase projects.",
    icon: <SupabaseIcon size={16} />,
    command: "npx -y @supabase/mcp-server-supabase",
    env: [{ key: "SUPABASE_ACCESS_TOKEN", label: "Personal access token", placeholder: "sbp_…" }],
    note: "needs a token",
  },
  {
    key: "mongodb",
    category: "Data & infra",
    name: "MongoDB",
    desc: "Official MongoDB server — query collections, inspect schemas.",
    icon: <MongoIcon size={16} />,
    command: "npx -y mongodb-mcp-server",
    env: [{ key: "MDB_MCP_CONNECTION_STRING", label: "Connection string", placeholder: "mongodb+srv://…" }],
    note: "needs a connection string",
  },
  {
    key: "airtable",
    category: "Data & infra",
    name: "Airtable",
    desc: "Read and update your bases.",
    icon: <AirtableIcon size={16} />,
    command: "npx -y airtable-mcp-server",
    env: [{ key: "AIRTABLE_API_KEY", label: "Personal access token", placeholder: "pat…" }],
    note: "community server · needs a token",
  },
  {
    key: "exa",
    category: "Web & search",
    name: "Exa search",
    desc: "Neural web search built for agents.",
    icon: <SearchLineIcon size={16} />,
    command: "npx -y exa-mcp-server",
    env: [{ key: "EXA_API_KEY", label: "Exa API key", placeholder: "…" }],
    note: "needs an API key",
  },
  {
    key: "duckduckgo",
    category: "Web & search",
    name: "DuckDuckGo",
    desc: "Keyless web search.",
    icon: <DuckDuckGoIcon size={16} />,
    command: "npx -y duckduckgo-mcp-server",
    note: "community server · no keys needed",
  },
  {
    key: "asana",
    category: "Productivity",
    name: "Asana",
    desc: "Tasks and projects — official remote server.",
    icon: <AsanaIcon size={16} />,
    command: "https://mcp.asana.com/sse",
    note: "sign in with /mcp inside Claude Code after adding",
  },
  {
    key: "atlassian",
    category: "Productivity",
    name: "Atlassian (Jira + Confluence)",
    desc: "Issues, boards, and pages — official remote server.",
    icon: <AtlassianIcon size={16} />,
    command: "https://mcp.atlassian.com/v1/sse",
    note: "sign in with /mcp inside Claude Code after adding",
  },
  {
    key: "vercel",
    category: "Dev & code",
    name: "Vercel",
    desc: "Deployments, projects, and logs — official remote server.",
    icon: <VercelIcon size={16} />,
    command: "https://mcp.vercel.com",
    note: "sign in with /mcp inside Claude Code after adding",
  },
  {
    key: "paypal",
    category: "Business",
    name: "PayPal",
    desc: "Commerce operations — official remote server.",
    icon: <PayPalIcon size={16} />,
    command: "https://mcp.paypal.com/mcp",
    note: "sign in with /mcp inside Claude Code after adding",
  },
];

export function CatalogRow({ entry, added, onDone, onErr }: { entry: CatalogEntry; added: boolean; onDone: (note?: string) => void; onErr: (e: string) => void }) {
  const [open, setOpen] = useState(false);
  const [vals, setVals] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const needs = (entry.env?.length ?? 0) + (entry.arg ? 1 : 0) > 0;
  const ready = (entry.env ?? []).every((e) => (vals[e.key] ?? "").trim()) && (!entry.arg || (vals.__arg ?? "").trim());
  const connect = () => {
    setBusy(true);
    const command = entry.arg ? `${entry.command} ${(vals.__arg ?? "").trim()}` : entry.command;
    const env = (entry.env ?? []).map((e) => [e.key, (vals[e.key] ?? "").trim()] as [string, string]);
    invoke<Record<string, string>>("add_mcp", { name: entry.key, command, env })
      .then((r) => {
        setBusy(false); setOpen(false); setVals({});
        const extras = ["opencode", "antigravity"].filter((h) => r?.[h] === "ok");
        onDone(extras.length ? `Added — also registered in ${extras.join(" + ")}.` : "");
      })
      .catch((e) => { setBusy(false); onErr(String(e)); });
  };
  return (
    <div style={{ border: "1px solid var(--bd)", borderRadius: 9, background: "var(--panel)", padding: "9px 10px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
        <span style={{ width: 26, height: 26, border: "1px solid var(--bd2)", borderRadius: 7, display: "inline-flex", alignItems: "center", justifyContent: "center", color: "var(--fg)", flexShrink: 0 }}>{entry.icon}</span>
        <div style={{ flex: 1, minWidth: 0, lineHeight: 1.35 }}>
          <div style={{ fontWeight: 700, fontSize: 12.5, color: "var(--fg)" }}>{entry.name}</div>
          <div style={{ fontSize: 10.5, color: "var(--fg3)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{entry.desc}</div>
        </div>
        {added ? (
          <span style={{ fontSize: 11, color: "var(--fg3)", flexShrink: 0 }}>added ✓</span>
        ) : needs && !open ? (
          <button onClick={() => setOpen(true)} style={{ flexShrink: 0, height: 26, padding: "0 10px", borderRadius: 7, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg)", fontSize: 11.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>Add…</button>
        ) : !needs ? (
          <button onClick={connect} disabled={busy} style={{ flexShrink: 0, height: 26, padding: "0 10px", borderRadius: 7, border: "none", background: "var(--btn)", color: "var(--btnfg)", fontSize: 11.5, fontWeight: 700, cursor: "pointer", fontFamily: "inherit", opacity: busy ? 0.6 : 1 }}>{busy ? "…" : "Add"}</button>
        ) : null}
      </div>
      {!added && open && (
        <div className="lectern-fadein" style={{ display: "flex", flexDirection: "column", gap: 6, marginTop: 8 }}>
          {(entry.env ?? []).map((e) => (
            <input key={e.key} type="password" value={vals[e.key] ?? ""} onChange={(ev) => setVals((v) => ({ ...v, [e.key]: ev.target.value }))}
              placeholder={`${e.label} (${e.placeholder})`} spellCheck={false} className="mono" style={{ ...ctrl, width: "100%" }} />
          ))}
          {entry.arg && (
            <input value={vals.__arg ?? ""} onChange={(ev) => setVals((v) => ({ ...v, __arg: ev.target.value }))}
              placeholder={`${entry.arg.label} (${entry.arg.placeholder})`} spellCheck={false} className="mono" style={{ ...ctrl, width: "100%" }} />
          )}
          <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <button onClick={connect} disabled={!ready || busy} style={{ flex: 1, height: 26, borderRadius: 7, border: "none", background: "var(--btn)", color: "var(--btnfg)", fontSize: 11.5, fontWeight: 700, cursor: ready ? "pointer" : "default", fontFamily: "inherit", opacity: !ready || busy ? 0.55 : 1 }}>{busy ? "…" : "Connect"}</button>
            <button onClick={() => setOpen(false)} style={{ height: 26, padding: "0 9px", borderRadius: 7, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg2)", fontSize: 11.5, cursor: "pointer", fontFamily: "inherit" }}>Cancel</button>
          </div>
          <div style={{ fontSize: 10, color: "var(--fg-ghost, var(--fg3))", lineHeight: 1.4 }}>Stored by Claude Code with this server's config on your machine — never sent to Lectern.</div>
        </div>
      )}
    </div>
  );
}



/* The Channels shelf — every entry is a REAL plugin in the official Claude Code
   marketplace (verified against the local plugin catalog). WhatsApp isn't
   offered by the ecosystem yet; this list grows as the marketplace does. */
export type ChannelEntry = {
  key: string; name: string; desc: string; icon: React.ReactNode;
  install: string; note?: string;
};
export const CHANNEL_LIBRARY: ChannelEntry[] = [
  {
    key: "telegram",
    name: "Telegram",
    desc: "Message your agent from your phone; get completion pings back. Pairing + allowlist built in.",
    icon: <TelegramIcon size={16} />,
    install: "claude plugin install telegram@claude-plugins-official",
  },
  {
    key: "discord",
    name: "Discord",
    desc: "Same bridge, Discord DMs — pairing and access control included.",
    icon: <DiscordIcon size={16} />,
    install: "claude plugin install discord@claude-plugins-official",
  },
  {
    key: "imessage",
    name: "iMessage",
    desc: "Native iMessage bridge (reads chat.db directly).",
    icon: <IMessageIcon size={16} />,
    install: "claude plugin install imessage@claude-plugins-official",
    note: "macOS only",
  },
  {
    key: "fakechat",
    name: "Fakechat (test)",
    desc: "A localhost web chat for trying the channel flow — no tokens, no accounts.",
    icon: <FlaskLineIcon size={16} />,
    install: "claude plugin install fakechat@claude-plugins-official",
  },
];

type ChannelStatus = { name: string; configured: boolean; allowed: number; pending: number; dm_policy: string };

function ChannelRow({ c, status }: { c: ChannelEntry; status?: ChannelStatus }) {
  const [copied, setCopied] = useState(false);
  const active = status?.configured ?? false;
  return (
    <div style={{ border: "1px solid var(--bd)", borderRadius: 9, background: "var(--panel)", padding: "10px 12px", display: "flex", alignItems: "center", gap: 10 }}>
      <span style={{ width: 28, height: 28, flexShrink: 0, border: "1px solid var(--bd2)", borderRadius: 7, display: "inline-flex", alignItems: "center", justifyContent: "center", color: "var(--fg)" }}>{c.icon}</span>
      <div style={{ flex: 1, minWidth: 0, lineHeight: 1.4 }}>
        <div style={{ fontWeight: 700, fontSize: 12.5 }}>{c.name}{c.note ? <span style={{ fontWeight: 500, fontSize: 10.5, color: "var(--fg3)" }}>  · {c.note}</span> : null}</div>
        <div style={{ fontSize: 10.5, color: "var(--fg3)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{active ? `paired · ${status?.allowed} allowed sender${status?.allowed === 1 ? "" : "s"}` : c.desc}</div>
      </div>
      {active ? (
        <span style={{ display: "inline-flex", alignItems: "center", gap: 6, flexShrink: 0, fontSize: 11, fontWeight: 600, color: "var(--fg)", border: "1px solid var(--bd)", borderRadius: 999, padding: "3px 10px" }}>
          <span style={{ width: 6, height: 6, borderRadius: "50%", background: ACCENT }} />Active
        </span>
      ) : (
        <button title={`Copy: ${c.install} — run it in a terminal, then /${c.key}:configure inside claude`}
          onClick={() => { navigator.clipboard?.writeText(c.install).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1500); }).catch(() => {}); }}
          style={{ flexShrink: 0, height: 26, padding: "0 10px", borderRadius: 7, border: "1px solid var(--bd)", background: "transparent", color: "var(--fg)", fontSize: 11.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>
          {copied ? "Copied ✓" : "Copy install"}
        </button>
      )}
    </div>
  );
}

/* the Connect library — every default MCP server and
   channel in one browsable page. Same quality bar as the Settings card: every
   server verified (npm registry / live endpoint) before listing, real marks or
   honest neutral icons, requirement-aware forms, truthful notes. */
export default function Connect({ mcp, onRefresh, onBack }: { mcp: McpServer[]; onRefresh: () => void; onBack: () => void }) {
  const [q, setQ] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [channels, setChannels] = useState<ChannelStatus[]>([]);
  useEffect(() => { invoke<ChannelStatus[]>("channels_status").then(setChannels).catch(() => {}); }, []);
  const needle = q.trim().toLowerCase();
  const hits = MCP_CATALOG.filter((c) => !needle || c.name.toLowerCase().includes(needle) || c.desc.toLowerCase().includes(needle) || c.key.includes(needle) || c.category.toLowerCase().includes(needle));
  const cats = [...new Set(MCP_CATALOG.map((c) => c.category))];
  const chanHits = CHANNEL_LIBRARY.filter((c) => !needle || c.name.toLowerCase().includes(needle) || c.desc.toLowerCase().includes(needle));
  return (
    <Scroll>
      <div style={{ maxWidth: 720, margin: "0 auto", padding: "24px 24px 48px" }}>
        <button onClick={onBack} style={{ display: "inline-flex", alignItems: "center", gap: 6, border: "none", background: "transparent", color: "var(--fg3)", fontSize: 12.5, fontWeight: 600, cursor: "pointer", fontFamily: "inherit", padding: 0, marginBottom: 14 }}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden><path d="m15 6-6 6 6 6" /></svg>
          Settings
        </button>
        <div style={{ fontSize: 22, fontWeight: 800, letterSpacing: "-0.02em" }}>Connect</div>
        <div style={{ fontSize: 13, color: "var(--fg2)", margin: "4px 0 16px", lineHeight: 1.55 }}>
          Everything your agents can plug into — {MCP_CATALOG.length} verified tool servers and {CHANNEL_LIBRARY.length} remote-access
          channels. One click registers a tool in every agent you have.
        </div>
        <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search tools and channels…" spellCheck={false}
          style={{ ...ctrl, width: "100%", height: 34, marginBottom: 16 }} />
        {msg && <div className="lectern-fadein" style={{ fontSize: 12, color: "var(--fg2)", marginBottom: 10 }}>{msg}</div>}
        {cats.map((cat) => {
          const rows = hits.filter((c) => c.category === cat);
          if (!rows.length) return null;
          return (
            <div key={cat} style={{ marginBottom: 18 }}>
              <div className="mono" style={{ fontSize: 11, fontWeight: 600, color: "var(--fg3)", letterSpacing: "0.06em", marginBottom: 8 }}>{cat.toUpperCase()}</div>
              <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
                {rows.map((c) => (
                  <CatalogRow key={c.key} entry={c} added={mcp.some((m) => m.name.toLowerCase() === c.key)} onDone={(note) => { setMsg(note || null); onRefresh(); }} onErr={setMsg} />
                ))}
              </div>
            </div>
          );
        })}
        {chanHits.length > 0 && (
          <div style={{ marginBottom: 18 }}>
            <div className="mono" style={{ fontSize: 11, fontWeight: 600, color: "var(--fg3)", letterSpacing: "0.06em", marginBottom: 8 }}>CHANNELS — REMOTE ACCESS</div>
            <div style={{ fontSize: 11.5, color: "var(--fg3)", lineHeight: 1.5, marginBottom: 8 }}>
              Messaging apps that can reach your agent (Claude Code channel plugins — every entry here is real and
              installable today; WhatsApp isn't offered by the ecosystem yet). Install in a terminal, configure with
              /name:configure, pair from your phone — Lectern shows live status.
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
              {chanHits.map((c) => <ChannelRow key={c.key} c={c} status={channels.find((s) => s.name === c.key)} />)}
            </div>
          </div>
        )}
        {needle && hits.length === 0 && chanHits.length === 0 && (
          <div style={{ fontSize: 12.5, color: "var(--fg3)" }}>Nothing matches "{q}" — add a custom server from Settings → Tools.</div>
        )}
        <div style={{ fontSize: 11, color: "var(--fg3)", lineHeight: 1.5, borderTop: "1px solid var(--bd2)", paddingTop: 12 }}>
          Every server above was verified against the npm registry or its live endpoint before listing. Tokens you
          enter are stored by Claude Code with the server's config on your machine — never sent to Lectern. Channel
          pairing approvals always happen in the CLI, never from a chat message.
        </div>
      </div>
    </Scroll>
  );
}
