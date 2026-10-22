import { BlockList, isIP } from "node:net";

export function validIpRule(rule: string): boolean {
  const [address, prefix, extra] = rule.trim().split("/");
  if (extra !== undefined) return false;
  const family = isIP(address);
  if (!family) return false;
  if (prefix === undefined) return true;
  if (!/^\d+$/.test(prefix)) return false;
  const bits = Number(prefix);
  return bits >= 0 && bits <= (family === 4 ? 32 : 128);
}

export function ipAllowed(ip: string | null, rules: string[]): boolean {
  if (rules.length === 0) return true;
  if (!ip || !isIP(ip)) return false;
  const list = new BlockList();
  for (const rule of rules) {
    const [address, prefix] = rule.split("/");
    const family = isIP(address) === 6 ? "ipv6" : "ipv4";
    if (prefix === undefined) list.addAddress(address, family);
    else list.addSubnet(address, Number(prefix), family);
  }
  return list.check(ip);
}

export function clientIp(headers: Headers): string | null {
  if (process.env.TRUST_PROXY !== "true") return null;
  // The proxy must discard client-supplied X-Real-IP and set its own value.
  const ip = headers.get("x-real-ip");
  return ip && isIP(ip) ? ip : null;
}
