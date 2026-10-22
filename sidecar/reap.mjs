import { readdirSync, readFileSync } from "node:fs";

export function parseStat(line) {
  const open = line.indexOf("(");
  const close = line.lastIndexOf(")");
  if (open < 0 || close < open) {
    return null;
  }
  const pid = Number(line.slice(0, open).trim());
  const comm = line.slice(open + 1, close);
  const [state] = line.slice(close + 2).split(" ");
  return Number.isInteger(pid) ? { pid, comm, state } : null;
}

export function chromiumProcesses(stats) {
  const chromium = stats.filter((stat) => stat && stat.comm.startsWith("chrome"));
  return {
    running: chromium.filter((stat) => stat.state !== "Z").map((stat) => stat.pid),
    zombies: chromium.filter((stat) => stat.state === "Z").length,
  };
}

function readStats() {
  let entries = [];
  try {
    entries = readdirSync("/proc").filter((name) => /^\d+$/.test(name));
  } catch {
    return [];
  }
  return entries.map((pid) => {
    try {
      return parseStat(readFileSync(`/proc/${pid}/stat`, "utf8"));
    } catch {
      return null;
    }
  });
}

export function reapChromium() {
  const found = chromiumProcesses(readStats());
  for (const pid of found.running) {
    try {
      process.kill(pid, "SIGKILL");
    } catch {
      continue;
    }
  }
  return found;
}
