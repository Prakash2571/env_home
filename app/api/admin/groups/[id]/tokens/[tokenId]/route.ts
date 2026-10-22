import { NextRequest, NextResponse } from "next/server";
import { adminFromRequest, sameOrigin } from "@/lib/auth";
import { db } from "@/lib/db";
import { jsonError, serverError } from "@/lib/http";
import { uuid } from "@/lib/validation";

type Context = { params: Promise<{ id: string; tokenId: string }> };

export async function DELETE(request: NextRequest, context: Context) {
  if (!adminFromRequest(request)) return jsonError("Unauthorized", 401);
  if (!sameOrigin(request)) return jsonError("Invalid origin", 403);
  const { id, tokenId } = await context.params;
  if (!uuid(id) || !uuid(tokenId)) return jsonError("Invalid key", 400);
  try {
    const result = await db().query(
      "DELETE FROM access_tokens WHERE id = $1 AND group_id = $2 RETURNING id", [tokenId, id]
    );
    if (!result.rows[0]) return jsonError("Key not found", 404);
    return NextResponse.json({ ok: true });
  } catch (error) { return serverError(error); }
}
