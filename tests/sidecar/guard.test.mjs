import { test } from "node:test";
import assert from "node:assert/strict";

import { consentHosts, refusal } from "../../sidecar/guard.mjs";

const hosts = consentHosts("");
const good = {
  consentUrl: "https://auth.dhan.co/login/consentApp-login?consentAppId=c-1",
  redirectUrl: "https://login.beonedge.in/dhan/callback",
  accountId: "9999999999",
  totp: "123456",
  pin: "123456",
};

test("only Dhan's own consent host is driven", () => {
  assert.deepEqual(hosts, ["auth.dhan.co"]);
  assert.equal(refusal(good, hosts), null);
  assert.match(refusal({ ...good, consentUrl: "https://evil.example/login" }, hosts), /must be https on auth\.dhan\.co/);
  assert.match(refusal({ ...good, consentUrl: "http://auth.dhan.co/login" }, hosts), /must be https/);
  assert.match(refusal({ ...good, consentUrl: "https://auth.dhan.co.evil.example/x" }, hosts), /must be https/);
  assert.match(refusal({ ...good, consentUrl: "not a url" }, hosts), /must be URLs/);
});

test("the redirect, the PIN and the code are checked before a browser opens", () => {
  assert.match(refusal({ ...good, redirectUrl: "javascript:alert(1)" }, hosts), /http or https/);
  assert.match(refusal({ ...good, totp: "12345" }, hosts), /six digits/);
  assert.match(refusal({ ...good, pin: "12 34" }, hosts), /pin is malformed/);
});

test("the host list can be widened for a test broker", () => {
  assert.deepEqual(consentHosts(" auth.dhan.co, Sandbox.Dhan.co "), ["auth.dhan.co", "sandbox.dhan.co"]);
});
