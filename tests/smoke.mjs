import assert from "node:assert/strict";
import pg from "pg";

const base = "http://127.0.0.1:3000";
const origin = base;

async function request(path, method = "GET", body, cookie, bearer) {
  const response = await fetch(base + path, {
    method,
    headers: {
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
      ...(method === "GET" ? {} : { Origin: origin }),
      ...(cookie ? { Cookie: cookie } : {}),
      ...(bearer ? { Authorization: `Bearer ${bearer}` } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  let data;
  try { data = await response.json(); } catch { data = null; }
  return { response, data };
}

for (let attempt = 0; attempt < 30; attempt++) {
  try {
    const response = await fetch(base + "/login");
    if (response.ok) break;
  } catch { /* server still starting */ }
  if (attempt === 29) throw new Error("Server did not start");
  await new Promise((resolve) => setTimeout(resolve, 1000));
}

const unauthorized = await request("/api/admin/groups");
assert.equal(unauthorized.response.status, 401);

const badLogin = await request("/api/auth/login", "POST", { password: "wrong" });
assert.equal(badLogin.response.status, 401);

const login = await request("/api/auth/login", "POST", { password: process.env.ADMIN_PASSWORD });
assert.equal(login.response.status, 200);
const cookie = login.response.headers.get("set-cookie")?.split(";")[0];
assert.ok(cookie);

const created = await request("/api/admin/groups", "POST", { name: "smoke-test" }, cookie);
assert.equal(created.response.status, 201);
const id = created.data.group.id;

const saved = await request(`/api/admin/groups/${id}/secrets`, "PUT", {
  secrets: [{ name: "API_KEY", value: "super-secret" }, { name: "EMPTY", value: "" }],
}, cookie);
assert.equal(saved.response.status, 200);

const detail = await request(`/api/admin/groups/${id}`, "GET", undefined, cookie);
assert.equal(detail.response.status, 200);
assert.equal(detail.data.secrets.find((secret) => secret.name === "API_KEY").value, "super-secret");

const pool = new pg.Pool({ connectionString: process.env.DATABASE_URL });
try {
  const stored = await pool.query("SELECT encrypted_value FROM secrets WHERE group_id = $1 AND name = 'API_KEY'", [id]);
  assert.ok(stored.rows[0].encrypted_value);
  assert.ok(!stored.rows[0].encrypted_value.includes("super-secret"));
} finally {
  await pool.end();
}

const issued = await request(`/api/admin/groups/${id}/tokens`, "POST", { label: "smoke-client" }, cookie);
assert.equal(issued.response.status, 201);
const key = issued.data.token;
const tokenId = issued.data.access.id;

const noKey = await request("/api/v1/groups/smoke-test");
assert.equal(noKey.response.status, 401);
const fetched = await request("/api/v1/groups/smoke-test", "GET", undefined, undefined, key);
assert.equal(fetched.response.status, 200);
assert.equal(fetched.data.variables.API_KEY, "super-secret");

const restricted = await request(`/api/admin/groups/${id}`, "PATCH", {
  name: "smoke-test", allowed_ips: ["127.0.0.1"],
}, cookie);
assert.equal(restricted.response.status, 200);
const denied = await request("/api/v1/groups/smoke-test", "GET", undefined, undefined, key);
assert.equal(denied.response.status, 403);

await request(`/api/admin/groups/${id}`, "PATCH", { name: "smoke-test", allowed_ips: [] }, cookie);
const revoked = await request(`/api/admin/groups/${id}/tokens/${tokenId}`, "DELETE", undefined, cookie);
assert.equal(revoked.response.status, 200);
const afterRevoke = await request("/api/v1/groups/smoke-test", "GET", undefined, undefined, key);
assert.equal(afterRevoke.response.status, 401);

console.log("Smoke test passed");
