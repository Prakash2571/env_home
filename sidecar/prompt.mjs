const KINDS = [
  ["totp", /\btotp\b|authenticator|authentication app|time[\s-]?based/i],
  ["pin", /\b(pin|t-?pin|m-?pin|passcode)\b/i],
  ["otp", /\botp\b|one[\s-]?time|sent to|\bsms\b/i],
  ["account", /mobile|phone|client\s*id|user\s*id|login\s*id|\bucc\b|e-?mail/i],
];

const REJECTION =
  /invalid|incorrect|wrong|too many|attempts? (exceeded|left|remaining)|try again|expired|blocked|locked|not match/i;

export function classify(text) {
  const value = String(text || "");
  for (const [kind, pattern] of KINDS) {
    if (pattern.test(value)) {
      return kind;
    }
  }
  return null;
}

export function classifyNearest(candidates) {
  for (const candidate of candidates) {
    const kind = classify(candidate);
    if (kind) {
      return { kind, prompt: shorten(candidate) };
    }
  }
  return { kind: null, prompt: shorten(candidates.find((candidate) => candidate) || "") };
}

export function rejection(text) {
  const lines = String(text || "")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line !== "" && line.length <= 200);
  return lines.find((line) => REJECTION.test(line)) || null;
}

export function lockedOut(message) {
  return /too many|locked|blocked|try again later/i.test(String(message || ""));
}

function shorten(text) {
  const single = String(text).replace(/\s+/g, " ").trim();
  return single.length > 120 ? `${single.slice(0, 117)}...` : single;
}
