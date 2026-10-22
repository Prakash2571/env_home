import { NextRequest, NextResponse } from "next/server";
import { decrypt, digest } from "@/lib/crypto";
import { db } from "@/lib/db";
import { jsonError, serverError } from "@/lib/http";
import { clientIp, ipAllowed } from "@/lib/ip";

type Context = { params: Promise<{ slug: string }> };

export async function GET(request: NextRequest, context: Context) {
  const authorization = request.headers.get("authorization");
  if (!authorization?.startsWith("Bearer ")) return jsonError("Unauthorized", 401);
  const token = authorization.slice(7);
  if (!/^eh_[A-Za-z0-9_-]{43}$/.test(token)) return jsonError("Unauthorized", 401);
  const { slug } = await context.params;
  try {
    const access = await db().query(
      `SELECT t.id, g.id AS group_id, g.allowed_ips
       FROM access_tokens t JOIN groups g ON g.id = t.group_id
       WHERE t.token_hash = $1 AND g.slug = $2`,
      [digest(token), slug]
    );
    const row = access.rows[0];
    if (!row) return jsonError("Unauthorized", 401);
    if (!ipAllowed(clientIp(request.headers), row.allowed_ips)) return jsonError("Forbidden", 403);
    const result = await db().query(
      "SELECT name, encrypted_value FROM secrets WHERE group_id = $1 ORDER BY name", [row.group_id]
    );
    const variables = Object.fromEntries(result.rows.map((item) => [
      item.name, decrypt(item.encrypted_value)
    ]));
    await db().query("UPDATE access_tokens SET last_used_at = now() WHERE id = $1", [row.id]);
    return NextResponse.json({ group: slug, variables }, { headers: { "Cache-Control": "no-store" } });
  } catch (error) { return serverError(error); }
}
