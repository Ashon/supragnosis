// The browser scenarios' harness (docs/e2e.md Section 3): a daemon in a home of its own, an MCP
// client that speaks to it as an AI app does, the console over its socket, and the proxy a browser
// reaches the viewer through. Every process it starts is stopped by the `stop` it returns (J5).
import { spawn } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";

/// The binary the scenarios run: a release build, as the viewer's assets ship (J1).
export const BIN =
  process.env.SUPRAGNOSIS_SCENARIO_BIN || path.resolve(import.meta.dirname, "../../target/release/supragnosis");

/// Polls `fn` until it returns a value, or fails with what it last saw once `ms` pass (J3).
export async function waitFor(what, ms, fn) {
  const deadline = Date.now() + ms;
  let last = "nothing yet";
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
      last = JSON.stringify(v);
    } catch (e) {
      last = String(e && e.message ? e.message : e);
    }
    if (Date.now() > deadline) throw new Error(`${what}: still ${last} after ${ms} ms`);
    await new Promise((r) => setTimeout(r, 50));
  }
}

function canConnect(sock) {
  return new Promise((resolve) => {
    const c = net.connect(sock, () => { c.end(); resolve(true); });
    c.on("error", () => resolve(false));
  });
}

/// Starts `supragnosis serve` on a port the system chooses, from an environment built for it (J2):
/// its own home, socket and embedder, and nothing inherited but a coverage profile's destination.
export async function startDaemon({ embed = "hashing", store = "redb" } = {}) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "sg-home-"));
  const sock = path.join(os.tmpdir(), `sg-${crypto.randomBytes(4).toString("hex")}.sock`);
  const env = { HOME: home, SUPRAGNOSIS_EMBED: embed, SUPRAGNOSIS_STORE: store };
  if (process.env.LLVM_PROFILE_FILE) env.LLVM_PROFILE_FILE = process.env.LLVM_PROFILE_FILE;
  const logPath = path.join(home, "daemon.log");
  const log = fs.openSync(logPath, "a");
  const child = spawn(BIN, ["serve", "--http", "127.0.0.1:0", "--viz", sock], { env, stdio: ["ignore", log, log] });
  const exited = new Promise((r) => child.on("exit", (code, signal) => r({ code, signal })));
  const listening = path.join(home, ".supragnosis", "listening");
  const addr = await waitFor("the daemon's address", 20_000, () => {
    const m = /^mcp (\S+)$/m.exec(fs.existsSync(listening) ? fs.readFileSync(listening, "utf8") : "");
    return m && m[1];
  });
  await waitFor("the viewer socket", 10_000, () => canConnect(sock));
  const token = fs.readFileSync(path.join(home, ".supragnosis", "mcp.token"), "utf8").trim();
  const daemon = {
    home, sock, addr, token, logPath,
    // SIGTERM is a normal exit (daemon-lifecycle.md); one that does not come in time is killed.
    async stop() {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill("SIGTERM");
        const done = await Promise.race([exited, new Promise((r) => setTimeout(() => r(null), 10_000))]);
        if (!done) { child.kill("SIGKILL"); await exited; }
      }
      fs.rmSync(home, { recursive: true, force: true });
      fs.rmSync(sock, { force: true });
    },
  };
  return daemon;
}

/// One request to the viewer over its socket, as the console makes it. Answers the status and the
/// body, parsed when it is JSON.
export function viz(daemon, target, method = "GET") {
  return new Promise((resolve, reject) => {
    const req = http.request({ socketPath: daemon.sock, path: target, method, headers: { host: "viz" } }, (res) => {
      let body = "";
      res.setEncoding("utf8");
      res.on("data", (c) => (body += c));
      res.on("end", () => {
        let json = null;
        try { json = JSON.parse(body); } catch (_) { /* not JSON */ }
        resolve({ status: res.statusCode, body: json ?? body });
      });
    });
    req.on("error", reject);
    req.end();
  });
}

/// The JSON-RPC messages in a server-sent-events body (the bridge's sse_messages, in JavaScript).
function sseMessages(body) {
  return body.replace(/\r\n/g, "\n").split("\n\n").map((event) => event.split("\n")
    .filter((l) => l.startsWith("data:")).map((l) => l.slice(5).replace(/^ /, "")).join("\n"))
    .filter((d) => d.trim()).map((d) => JSON.parse(d));
}

/// An MCP client as an AI app speaks it: streamable HTTP with the bearer token, one session.
export class Mcp {
  constructor(daemon, name = "scenario-agent") {
    this.daemon = daemon;
    this.name = name;
    this.session = null;
    this.protocol = null;
    this.id = 0;
  }

  async post(msg) {
    const headers = {
      accept: "application/json, text/event-stream",
      "content-type": "application/json",
      authorization: `Bearer ${this.daemon.token}`,
    };
    if (this.session) headers["mcp-session-id"] = this.session;
    if (this.protocol) headers["mcp-protocol-version"] = this.protocol;
    const res = await fetch(`http://${this.daemon.addr}/mcp`, { method: "POST", headers, body: JSON.stringify(msg) });
    if (res.headers.get("mcp-session-id")) this.session = res.headers.get("mcp-session-id");
    const text = await res.text();
    if (res.status === 202) return [];
    if (res.status !== 200) throw new Error(`MCP ${msg.method}: ${res.status} ${text.slice(0, 200)}`);
    return (res.headers.get("content-type") || "").startsWith("text/event-stream") ? sseMessages(text) : [JSON.parse(text)];
  }

  async rpc(method, params) {
    const id = ++this.id;
    const replies = await this.post({ jsonrpc: "2.0", id, method, params });
    const reply = replies.find((r) => r.id === id);
    if (!reply) throw new Error(`MCP ${method}: no reply`);
    if (reply.error) throw new Error(`MCP ${method}: ${JSON.stringify(reply.error)}`);
    return reply.result;
  }

  async init() {
    const r = await this.rpc("initialize", {
      protocolVersion: "2025-06-18",
      capabilities: {},
      clientInfo: { name: this.name, version: "1.0.0" },
    });
    this.protocol = r.protocolVersion;
    await this.post({ jsonrpc: "2.0", method: "notifications/initialized" });
    return this;
  }

  /// A tool call; its text answer parsed as JSON. A tool error is thrown, not returned.
  async tool(name, args) {
    const r = await this.rpc("tools/call", { name, arguments: args });
    const text = r.content && r.content[0] && r.content[0].text;
    if (r.isError) throw new Error(`${name}: ${text}`);
    try { return JSON.parse(text); } catch (_) { return text; }
  }
}

/// The proxy a browser reaches the viewer through, for a scenario's lifetime only (Section 3.2, J6):
/// a loopback port the system chooses, and nothing answered for a foreign Host or Origin. Event
/// streams pass through as they come.
export async function startProxy(daemon) {
  const server = http.createServer((req, res) => {
    const own = `127.0.0.1:${server.address().port}`;
    if (req.headers.host !== own) { res.writeHead(421).end("not this proxy's host"); return; }
    if (req.headers.origin && req.headers.origin !== `http://${own}`) { res.writeHead(403).end("foreign origin"); return; }
    const up = http.request({ socketPath: daemon.sock, path: req.url, method: req.method, headers: { ...req.headers, host: "viz" } }, (u) => {
      res.writeHead(u.statusCode, u.headers);
      u.pipe(res);
    });
    up.on("error", (e) => { if (!res.headersSent) res.writeHead(502); res.end(String(e)); });
    req.pipe(up);
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const url = `http://127.0.0.1:${server.address().port}/`;
  return { url, close: () => new Promise((r) => { server.closeAllConnections(); server.close(() => r()); }) };
}
