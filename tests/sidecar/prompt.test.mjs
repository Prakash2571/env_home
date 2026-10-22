import { test } from "node:test";
import assert from "node:assert/strict";

import { classify, classifyNearest, lockedOut, rejection } from "../../sidecar/prompt.mjs";

test("each box is recognised by what Dhan writes around it", () => {
  assert.equal(classify("Enter TOTP"), "totp");
  assert.equal(classify("Enter the 6 digit code from your authenticator app"), "totp");
  assert.equal(classify("Enter your 6 digit PIN"), "pin");
  assert.equal(classify("Enter OTP sent to +91 98xxxx1234"), "otp");
  assert.equal(classify("Mobile Number"), "account");
  assert.equal(classify("Client ID"), "account");
  assert.equal(classify("Generate Access"), null);
});

test("a TOTP box beside a link offering SMS OTP is still a TOTP box", () => {
  assert.equal(classify("Enter TOTP | Login with OTP instead"), "totp");
});

test("the PIN is never read into a code field and a code never into a PIN field", () => {
  assert.notEqual(classify("Enter TOTP"), "pin");
  assert.notEqual(classify("Enter your PIN"), "totp");
  assert.equal(classify("Pincode"), null, "a postal pincode is not the account PIN");
});

test("the nearest text that names a kind decides, not a heading further out", () => {
  const found = classifyNearest(["", "Enter PIN", "Login via your Dhan account | Enter TOTP"]);
  assert.deepEqual(found, { kind: "pin", prompt: "Enter PIN" });
  assert.equal(classifyNearest(["", "Something new"]).kind, null);
  assert.equal(classifyNearest(["", "Something new"]).prompt, "Something new");
});

test("Dhan's refusals are recognised and the page's own furniture is not", () => {
  assert.equal(
    rejection("Enter TOTP\nToo many attempts. Please try after some time\nContinue"),
    "Too many attempts. Please try after some time",
  );
  assert.equal(rejection("Invalid TOTP"), "Invalid TOTP");
  assert.equal(rejection("Generate Access\nLogin via your Dhan account\nContinue"), null);
});

test("a lockout is told apart from an ordinary wrong code", () => {
  assert.equal(lockedOut("Dhan said: Too many attempts"), true);
  assert.equal(lockedOut("Dhan said: Invalid TOTP"), false);
});
