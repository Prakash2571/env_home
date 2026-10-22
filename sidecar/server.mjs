import { createServer } from "node:http";

import { consentHosts, refusal } from "./guard.mjs";
import { runLogin } from "./login.mjs";
import { reapChromium } from "./reap.mjs";

const PORT = Number(process.env.PORT || 8090);
const ADDRESS = process.env.ADDRESS || "0.0.0.0";
const MAX_BODY_BYTES = 8192;
const REQUIRED = ["consentUrl", "redirectUrl", "accountId", "totp", "pin"];
const CONSENT_HOSTS = consentHosts(process.env.CONSENT_HOSTS);

let busy = false;

function reply(response, status, body) {
  const payload = JSON.stringify(body);
  response.writeHead(status, {
    "content-type": "application/json",
    "cache-control": "no-store",
    "content-length": Buffer.byteLength(payload),
  });
  response.end(payload);
}

function readBody(request) {
  return new Promise((resolve, reject) => {
    let size = 0;
    const chunks = [];
    request.on("data", (chunk) => {
      size += chunk.length;
      if (size > MAX_BODY_BYTES) {
        reject(new Error("request body exceeded the size limit"));
        request.destroy();
        return;
      }
      chunks.push(chunk);
    });
    request.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    request.on("error", reject);
  });
}

function redact(request) {
  return {
    consentUrl: request.consentUrl,
    redirectUrl: request.redirectUrl,
    accountId: `${String(request.accountId).slice(0, 2)}***`,
  };
}

const server = createServer(async (request, response) => {
  if (request.method === "GET" && request.url === "/health") {
    const chromium = busy ? null : reapChromium();
    reply(response, 200, { alive: true, busy, chromium });
    return;
  }

  if (request.method !== "POST" || request.url !== "/login") {
    reply(response, 404, { ok: false, stage: "route", error: "POST /login only" });
    return;
  }

  if (busy) {
    reply(response, 409, {
      ok: false,
      stage: "busy",
      error: "a browser login is already running; only one fits in memory",
    });
    return;
  }

  let parsed;
  try {
    parsed = JSON.parse(await readBody(request));
  } catch (error) {
    reply(response, 400, { ok: false, stage: "request", error: String(error.message) });
    return;
  }

  const missing = REQUIRED.filter(
    (key) => typeof parsed[key] !== "string" || parsed[key].trim() === "",
  );
  if (missing.length > 0) {
    reply(response, 400, {
      ok: false,
      stage: "request",
      error: `missing or empty: ${missing.join(", ")}`,
    });
    return;
  }

  const refused = refusal(parsed, CONSENT_HOSTS);
  if (refused !== null) {
    reply(response, 400, { ok: false, stage: "request", error: refused });
    return;
  }

  busy = true;
  const startedAt = Date.now();
  console.log(`login requested ${JSON.stringify(redact(parsed))}`);

  try {
    const outcome = await runLogin(parsed);
    const seconds = ((Date.now() - startedAt) / 1000).toFixed(1);
    console.log(
      outcome.ok
        ? `login succeeded at ${outcome.stage} in ${seconds}s`
        : `login failed at ${outcome.stage} in ${seconds}s: ${outcome.error}`,
    );
    reply(response, outcome.ok ? 200 : 502, outcome);
  } catch (error) {
    console.error(`login threw: ${error && error.stack ? error.stack : error}`);
    reply(response, 500, { ok: false, stage: "sidecar", error: String(error) });
  } finally {
    const left = reapChromium();
    if (left.running.length > 0 || left.zombies > 0) {
      console.log(
        `chromium left ${left.running.length} running (killed) and ${left.zombies} unreaped after the login`,
      );
    }
    busy = false;
  }
});

server.headersTimeout = 20_000;
server.requestTimeout = 300_000;

server.listen(PORT, ADDRESS, () => {
  console.log(`Dhan login sidecar listening on ${ADDRESS}:${PORT}`);
});

for (const signal of ["SIGTERM", "SIGINT"]) {
  process.on(signal, () => {
    server.close(() => process.exit(0));
  });
}
