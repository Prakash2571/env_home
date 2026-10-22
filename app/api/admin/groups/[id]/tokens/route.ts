import { randomBytes } from "node:crypto";
import { NextRequest, NextResponse } from "next/server";
import { adminFromRequest, sameOrigin } from "@/lib/auth";
import { digest } from "@/lib/crypto";
import { db } from "@/lib/db";
import { jsonBody, jsonError, serverError } from "@/lib/http";
import { uuid } from "@/lib/validation";

type Context = { params: Promise<{ id: string }> };

export async function POST(request: NextRequest, context: Context) {
  if (!adminFromRequest(request)) return jsonError("Unauthorized", 401);
  if (!sameOrigin(request)) return jsonError("Invalid origin", 403);
  const { id } = await context.params;
  if (!uuid(id)) return jsonError("Invalid group", 400);
  const body = await jsonBody(request) as { label?: unknown } | null;
  const label = typeof body?.label === "string" ? body.label.trim() : "";
  if (!label || label.length > 80) return jsonError("Key label must be 1 to 80 characters", 400);
  const token = `eh_${randomBytes(32).toString("base64url")}`;
  try {
    const result = await db().query(
      "INSERT INTO access_tokens (group_id, label, token_hash) VALUES ($1, $2, $3) RETURNING id, label, created_at",
      [id, label, digest(token)]
    );
    return NextResponse.json({ token, access: result.rows[0] }, {
      status: 201, headers: { "Cache-Control": "no-store" }
    });
  } catch (error) {
    if ((error as { code?: string }).code === "23503") return jsonError("Group not found", 404);
    return serverError(error);
  }
}
