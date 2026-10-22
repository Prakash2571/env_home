import { NextRequest, NextResponse } from "next/server";
import { adminFromRequest, sameOrigin } from "@/lib/auth";
import { decrypt } from "@/lib/crypto";
import { db } from "@/lib/db";
import { jsonBody, jsonError, serverError } from "@/lib/http";
import { groupName, groupSlug, ipRules, uuid } from "@/lib/validation";

type Context = { params: Promise<{ id: string }> };

export async function GET(request: NextRequest, context: Context) {
  if (!adminFromRequest(request)) return jsonError("Unauthorized", 401);
  const { id } = await context.params;
  if (!uuid(id)) return jsonError("Invalid group", 400);
  try {
    const [group, secrets, tokens] = await Promise.all([
      db().query("SELECT id, name, slug, allowed_ips FROM groups WHERE id = $1", [id]),
      db().query("SELECT name, encrypted_value FROM secrets WHERE group_id = $1 ORDER BY name", [id]),
      db().query("SELECT id, label, created_at, last_used_at FROM access_tokens WHERE group_id = $1 ORDER BY created_at DESC", [id]),
    ]);
    if (!group.rows[0]) return jsonError("Group not found", 404);
    return NextResponse.json({
      group: group.rows[0],
      secrets: secrets.rows.map((row) => ({ name: row.name, value: decrypt(row.encrypted_value) })),
      tokens: tokens.rows,
    }, { headers: { "Cache-Control": "no-store" } });
  } catch (error) { return serverError(error); }
}

export async function PATCH(request: NextRequest, context: Context) {
  if (!adminFromRequest(request)) return jsonError("Unauthorized", 401);
  if (!sameOrigin(request)) return jsonError("Invalid origin", 403);
  const { id } = await context.params;
  if (!uuid(id)) return jsonError("Invalid group", 400);
  const body = await jsonBody(request) as { name?: unknown; allowed_ips?: unknown } | null;
  const name = groupName(body?.name);
  const allowedIps = ipRules(body?.allowed_ips);
  if (!name || !allowedIps) return jsonError("Invalid name or IP rules", 400);
  const slug = groupSlug(name);
  if (!slug) return jsonError("Name must contain a letter or number", 400);
  try {
    const result = await db().query(
      "UPDATE groups SET name = $1, slug = $2, allowed_ips = $3 WHERE id = $4 RETURNING id, name, slug, allowed_ips",
      [name, slug, allowedIps, id]
    );
    if (!result.rows[0]) return jsonError("Group not found", 404);
    return NextResponse.json({ group: result.rows[0] });
  } catch (error) {
    if ((error as { code?: string }).code === "23505") return jsonError("A group with that name already exists", 409);
    return serverError(error);
  }
}

export async function DELETE(request: NextRequest, context: Context) {
  if (!adminFromRequest(request)) return jsonError("Unauthorized", 401);
  if (!sameOrigin(request)) return jsonError("Invalid origin", 403);
  const { id } = await context.params;
  if (!uuid(id)) return jsonError("Invalid group", 400);
  try {
    const result = await db().query("DELETE FROM groups WHERE id = $1 RETURNING id", [id]);
    if (!result.rows[0]) return jsonError("Group not found", 404);
    return NextResponse.json({ ok: true });
  } catch (error) { return serverError(error); }
}
