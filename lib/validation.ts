import { validIpRule } from "@/lib/ip";

export type EnvPair = { name: string; value: string };

export function groupName(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const name = value.trim();
  return name.length >= 1 && name.length <= 80 ? name : null;
}

export function groupSlug(name: string): string {
  return name.toLowerCase().normalize("NFKD").replace(/[\u0300-\u036f]/g, "")
    .replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 64);
}

export function envPairs(value: unknown): EnvPair[] | null {
  if (!Array.isArray(value) || value.length > 500) return null;
  const seen = new Set<string>();
  for (const pair of value) {
    if (!pair || typeof pair !== "object" || typeof pair.name !== "string" ||
      typeof pair.value !== "string" || !/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(pair.name) ||
      pair.value.length > 65536 || seen.has(pair.name)) return null;
    seen.add(pair.name);
  }
  return value as EnvPair[];
}

export function ipRules(value: unknown): string[] | null {
  if (!Array.isArray(value) || value.length > 50 ||
    !value.every((rule) => typeof rule === "string" && validIpRule(rule))) return null;
  return [...new Set(value.map((rule: string) => rule.trim()))];
}

export function uuid(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
}
