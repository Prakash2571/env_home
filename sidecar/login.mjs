import { chromium } from "playwright";

import { classifyNearest, rejection } from "./prompt.mjs";

const NAVIGATION_TIMEOUT_MS = 45_000;
const SETTLE_TIMEOUT_MS = 12_000;
const MAX_STEPS = 12;
const HEADED = process.env.DHAN_BROWSER_HEADED === "true";
const SLOW_MO_MS = Number(process.env.DHAN_BROWSER_SLOW_MO_MS || 0);
const BLOCKED_RESOURCES = HEADED
  ? new Set(["font", "media"])
  : new Set(["image", "font", "media"]);
const SPLIT_GROUP_MIN = 4;

const LAUNCH_ARGS = [
  "--disable-dev-shm-usage",
  "--no-sandbox",
  "--disable-gpu",
  "--disable-extensions",
  "--disable-background-networking",
  "--renderer-process-limit=1",
];

export function tokenFrom(url, redirectPath) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.pathname.replace(/\/+$/, "") !== redirectPath) {
    return null;
  }
  const token = parsed.searchParams.get("tokenId");
  return token && token.trim() !== "" ? token : null;
}

function redirectPathOf(redirectUrl) {
  return new URL(redirectUrl).pathname.replace(/\/+$/, "");
}

async function emptyVisibleInputs(page) {
  const handles = await page.locator("input:visible").all();
  const usable = [];
  for (const handle of handles) {
    const type = ((await handle.getAttribute("type")) || "text").toLowerCase();
    if (["hidden", "checkbox", "radio", "submit", "button", "file"].includes(type)) {
      continue;
    }
    if (await handle.isDisabled()) {
      continue;
    }
    const value = await handle.inputValue().catch(() => "x");
    if (value !== "") {
      continue;
    }
    const maxLength = Number((await handle.getAttribute("maxlength")) || 0);
    usable.push({ handle, maxLength });
  }
  return usable;
}

async function promptCandidates(handle) {
  return handle.evaluate((element) => {
    const own = [
      element.getAttribute("aria-label"),
      element.getAttribute("placeholder"),
      element.getAttribute("name"),
      element.id,
    ].filter((value) => value);
    const labels = element.labels ? Array.from(element.labels, (label) => label.innerText) : [];
    const around = [];
    let node = element.parentElement;
    for (let depth = 0; node && depth < 6; depth += 1, node = node.parentElement) {
      const text = (node.innerText || "").trim();
      if (text !== "") {
        around.push(text.slice(0, 400));
      }
    }
    return [...own, ...labels, ...around];
  });
}

async function nextField(page) {
  const inputs = await emptyVisibleInputs(page);
  if (inputs.length === 0) {
    return null;
  }
  const split = inputs.filter((input) => input.maxLength === 1);
  const boxes = split.length >= SPLIT_GROUP_MIN ? split : [inputs[0]];
  const { kind, prompt } = classifyNearest(await promptCandidates(boxes[0].handle));
  const fill = async (value) => {
    if (boxes.length > 1 && boxes.length >= value.length) {
      for (let index = 0; index < value.length; index += 1) {
        await boxes[index].handle.fill(value[index]);
      }
      return;
    }
    await boxes[0].handle.fill(value);
  };
  return { kind, prompt, fill };
}

async function visibleText(page) {
  return page.locator("body").innerText().catch(() => "");
}

async function submit(page) {
  const labels = [
    "Continue",
    "Proceed",
    "Login",
    "Log in",
    "Sign in",
    "Submit",
    "Verify",
    "Authorise",
    "Authorize",
    "Allow",
    "Next",
  ];
  for (const label of labels) {
    const button = page
      .getByRole("button", { name: new RegExp(`^\\s*${label}\\s*$`, "i") })
      .first();
    if (await button.isVisible().catch(() => false)) {
      if (await button.isEnabled().catch(() => false)) {
        await button.click({ timeout: SETTLE_TIMEOUT_MS }).catch(() => {});
        return true;
      }
    }
  }
  const fallback = page.locator("button:visible, input[type=submit]:visible").first();
  if (await fallback.isVisible().catch(() => false)) {
    await fallback.click({ timeout: SETTLE_TIMEOUT_MS }).catch(() => {});
    return true;
  }
  await page.keyboard.press("Enter").catch(() => {});
  return false;
}

async function settle(page) {
  await page
    .waitForLoadState("domcontentloaded", { timeout: SETTLE_TIMEOUT_MS })
    .catch(() => {});
  await page.waitForTimeout(1200);
}

export async function runLogin({ consentUrl, redirectUrl, accountId, totp, pin }) {
  const redirectPath = redirectPathOf(redirectUrl);
  let stage = "launch";
  let browser;

  try {
    browser = await chromium.launch({
      headless: !HEADED,
      slowMo: SLOW_MO_MS,
      args: LAUNCH_ARGS,
    });
    const context = await browser.newContext({
      viewport: { width: 1280, height: 900 },
      serviceWorkers: "block",
    });
    context.setDefaultTimeout(SETTLE_TIMEOUT_MS);
    context.setDefaultNavigationTimeout(NAVIGATION_TIMEOUT_MS);

    let captured = null;
    await context.route("**/*", (route) => {
      const request = route.request();
      const found = tokenFrom(request.url(), redirectPath);
      if (found) {
        captured = found;
        return route.abort();
      }
      if (BLOCKED_RESOURCES.has(request.resourceType())) {
        return route.abort();
      }
      return route.continue();
    });

    const page = await context.newPage();

    stage = "consent";
    await page.goto(consentUrl, { waitUntil: "domcontentloaded" });

    const values = { account: accountId, totp, pin };
    const entered = [];

    for (let step = 0; step < MAX_STEPS; step += 1) {
      if (captured) {
        break;
      }
      const direct = tokenFrom(page.url(), redirectPath);
      if (direct) {
        captured = direct;
        break;
      }

      if (entered.length > 0) {
        const refused = rejection(await visibleText(page));
        if (refused) {
          const last = entered[entered.length - 1];
          return { ok: false, stage: `${last}-rejected`, error: `Dhan said: ${refused}` };
        }
      }

      const field = await nextField(page);
      if (field) {
        if (field.kind === "otp") {
          return {
            ok: false,
            stage: "otp",
            error: `Dhan asked for an SMS OTP ("${field.prompt}"), which only a person can read; log in from the console link`,
          };
        }
        if (field.kind === null) {
          return {
            ok: false,
            stage: "unknown-field",
            error: `an empty box asked for something unrecognised ("${field.prompt}"); nothing was typed into it`,
          };
        }
        if (entered.includes(field.kind)) {
          return {
            ok: false,
            stage: field.kind,
            error: `Dhan asked for the ${field.kind} again after it was entered; stopping rather than sending it twice`,
          };
        }
        stage = field.kind;
        await field.fill(values[field.kind]);
        entered.push(field.kind);
      } else {
        stage = "authorise";
      }

      await submit(page);
      await settle(page);
    }

    if (!captured) {
      const url = page.url();
      const title = await page.title().catch(() => "");
      const visible = (await page.locator("body").innerText().catch(() => ""))
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line !== "")
        .slice(0, 12)
        .join(" | ");
      return {
        ok: false,
        stage,
        error: `no tokenId reached ${redirectPath}; last url ${url}; title ${title}; page ${visible}`,
      };
    }

    return { ok: true, tokenId: captured, stage: "callback" };
  } catch (error) {
    return { ok: false, stage, error: String(error && error.message ? error.message : error) };
  } finally {
    if (browser) {
      await browser.close().catch(() => {});
    }
  }
}
