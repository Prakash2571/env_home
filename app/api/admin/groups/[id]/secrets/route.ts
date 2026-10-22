import { NextRequest, NextResponse } from "next/server";
import { adminFromRequest, sameOrigin } from "@/lib/auth";
import { encrypt } from "@/lib/crypto";
import { db } from "@/lib/db";
import { jsonBody, jsonError, serverError } from "@/lib/http";
import { envPairs, uuid } from "@/lib/validation";

type Context = { params: Promise<{ id: string }> };

export async function PUT(request: NextRequest, context: Context) {
  if (!adminFromRequest(request)) return jsonError("Unauthorized", 401);
  if (!sameOrigin(request)) return jsonError("Invalid origin", 403);
  const { id } = await context.params;
  if (!uuid(id)) return jsonError("Invalid group", 400);
  const body = await jsonBody(request) as { secrets?: unknown } | null;
  const pairs = envPairs(body?.secrets);
  if (!pairs) return jsonError("Use unique env names and values up to 64 KiB", 400);
  const client = await db().connect();
  try {
    await client.query("BEGIN");
    const group = await client.query("SELECT id FROM groups WHERE id = $1 FOR UPDATE", [id]);
    if (!group.rows[0]) { await client.query("ROLLBACK"); return jsonError("Group not found", 404); }
    await client.query("DELETE FROM secrets WHERE group_id = $1", [id]);
    for (const pair of pairs) {
      await client.query(
        "INSERT INTO secrets (group_id, name, encrypted_value) VALUES ($1, $2, $3)",
        [id, pair.name, encrypt(pair.value)]
      );
    }
    await client.query("COMMIT");
    return NextResponse.json({ ok: true, count: pairs.length });
  } catch (error) {
    await client.query("ROLLBACK");
    return serverError(error);
  } finally { client.release(); }
}
