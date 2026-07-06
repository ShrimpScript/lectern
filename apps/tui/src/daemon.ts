/* lecternd JSON-RPC client (line-delimited over the unix socket).
   runSession streams AgentEvents via onEvent and resolves with the summary;
   cancel() uses a second connection + the run_id from the `started` note. */
import { connect } from "node:net";

export type AgentEvent = { type: string } & Record<string, unknown>;
export type RunParams = {
  path: string;
  prompt: string;
  backend: string;
  model?: string;
  apply: boolean;
  yolo: boolean;
  conduct: boolean;
};
export type RunResult = Record<string, unknown> & { error?: string };

const WIN = process.platform === "win32";
const home = process.env.HOME ?? process.env.USERPROFILE ?? ".";

export function socketPath(): string {
  const base = process.env.XDG_RUNTIME_DIR ?? "/tmp";
  return `${base}/lectern/lecternd.sock`;
}

/* Windows transport: 127.0.0.1 + port/token sidecar files
   written by lecternd; every request carries the token. */
function winMeta(): { port: number; token: string } | null {
  try {
    const fs = require("node:fs") as typeof import("node:fs");
    const port = parseInt(fs.readFileSync(`${home}/.lectern/lecternd.port`, "utf8").trim(), 10);
    const token = fs.readFileSync(`${home}/.lectern/lecternd.token`, "utf8").trim();
    return Number.isFinite(port) ? { port, token } : null;
  } catch {
    return null;
  }
}

function connectAny(): { sock: ReturnType<typeof connect>; token?: string } {
  if (WIN) {
    const meta = winMeta();
    if (!meta) throw new Error("lecternd port/token files not found — is the daemon running?");
    return { sock: connect(meta.port, "127.0.0.1"), token: meta.token };
  }
  return { sock: connect(socketPath()) };
}

function rpcOnce(method: string, params: unknown): Promise<RunResult> {
  return new Promise((resolve, reject) => {
    const { sock, token } = connectAny();
    let buf = "";
    sock.on("error", reject);
    sock.on("connect", () => {
      sock.write(JSON.stringify({ jsonrpc: "2.0", id: 1, method, params, ...(token ? { token } : {}) }) + "\n");
    });
    sock.on("data", (d) => {
      buf += d.toString();
      const nl = buf.indexOf("\n");
      if (nl >= 0) {
        try {
          const msg = JSON.parse(buf.slice(0, nl));
          resolve(msg.result ?? {});
        } catch (e) {
          reject(e);
        }
        sock.end();
      }
    });
  });
}

export const ping = () => rpcOnce("ping", {}).then(() => true).catch(() => false);

/** Capability probe: a live socket is not enough — a stale phase-0 daemon
    answers ping but knows no session methods (bit us in the wild: a June-26
    release binary produced "unknown method: run"). */
export const capable = () =>
  rpcOnce("models", {})
    .then((r) => !(r && typeof r === "object" && "error" in (r as Record<string, unknown>)))
    .catch(() => false);
export const cancelRun = (runId: string) => rpcOnce("cancel", { run_id: runId }).catch(() => ({}));

export function runSession(
  params: RunParams,
  onStarted: (runId: string) => void,
  onEvent: (ev: AgentEvent) => void,
): Promise<RunResult> {
  return new Promise((resolve, reject) => {
    const { sock, token } = connectAny();
    let buf = "";
    sock.on("error", reject);
    sock.on("connect", () => {
      sock.write(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "run", params, ...(token ? { token } : {}) }) + "\n");
    });
    sock.on("data", (d) => {
      buf += d.toString();
      let nl: number;
      while ((nl = buf.indexOf("\n")) >= 0) {
        const line = buf.slice(0, nl);
        buf = buf.slice(nl + 1);
        if (!line.trim()) continue;
        let msg: any;
        try {
          msg = JSON.parse(line);
        } catch {
          continue;
        }
        if (msg.method === "started") onStarted(String(msg.params?.run_id ?? ""));
        else if (msg.method === "event") onEvent(msg.params as AgentEvent);
        else if ("result" in msg) {
          resolve(msg.result as RunResult);
          sock.end();
        }
      }
    });
    sock.on("close", () => reject(new Error("daemon connection closed")));
  });
}

/** Start lecternd if the socket doesn't answer — PATH first, then the dev build. */
export async function ensureDaemon(): Promise<boolean> {
  if (await capable()) return true;
  if (await ping()) {
    console.error("lecternd is running but STALE (no session methods) — kill it and rebuild: cargo build -p lecternd");
    return false;
  }
  const candidates = [
    process.env.LECTERND_BIN,
    Bun.which("lecternd") ?? undefined,
    new URL("../../../target/release/lecternd", import.meta.url).pathname,
    new URL("../../../target/debug/lecternd", import.meta.url).pathname,
  ].filter(Boolean) as string[];
  for (const bin of candidates) {
    try {
      Bun.spawn([bin], { stdout: "ignore", stderr: "ignore", stdin: "ignore" });
      for (let i = 0; i < 10; i++) {
        await new Promise((r) => setTimeout(r, 200));
        if (await capable()) return true;
      }
      // spawned something that pings but can't serve sessions → stale binary;
      // fall through and try the next candidate (a fresh build usually).
    } catch {
      /* next candidate */
    }
  }
  return capable();
}

/* ── F1 read-only surface (sessions / history / models / skills / brain) ── */
export type SessionMeta = { id: string; title: string; backend: string; created_at: number; status: string; pinned?: boolean; meta?: null | { model?: string; view?: string; project?: string } };
export type ModelInfo = { id: string; label: string; backend: string };
export const listSessions = (path: string, limit = 30) =>
  rpcOnce("sessions", { path, limit }) as unknown as Promise<SessionMeta[]>;
export const sessionHistory = (session_id: string) =>
  rpcOnce("history", { session_id }) as unknown as Promise<AgentEvent[]>;
export const listModels = () =>
  rpcOnce("models", {}) as unknown as Promise<{ claude: ModelInfo[]; opencode: ModelInfo[] }>;
export const listSkills = (path: string) =>
  rpcOnce("skills", { path }) as unknown as Promise<{ name: string; description: string; uses: number; triggers: string[] }[]>;
export const renameSession = (sessionId: string, title: string) =>
  rpcOnce("session_rename", { session_id: sessionId, title }) as Promise<{ ok?: boolean; error?: string }>;
export const pinSession = (sessionId: string, pinned: boolean) =>
  rpcOnce("session_pin", { session_id: sessionId, pinned }) as Promise<{ ok?: boolean; pinned?: boolean; error?: string }>;
export const usageStats = () =>
  rpcOnce("usage", {}) as Promise<{
    days?: { day: string; input: number; output: number }[];
    backends?: { backend: string; input: number; output: number }[];
    total_input?: number; total_output?: number; error?: string;
  }>;
export const mcpOverview = () =>
  rpcOnce("mcp_overview", {}) as Promise<{ claude?: string[]; opencode?: string[]; antigravity?: string[] }>;
export const brainStats = (path: string) =>
  rpcOnce("brain", { path }) as unknown as Promise<{ sessions: number; skills: number; graph: boolean }>;
