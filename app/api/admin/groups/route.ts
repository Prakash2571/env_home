import { NextRequest, NextResponse } from "next/server";
import { adminFromRequest, sameOrigin } from "@/lib/auth";
import { db } from "@/lib/db";
import { jsonBody, jsonError, serverError } from "@/lib/http";
import { groupName, groupSlug } from "@/lib/validation";

export async function GET(request: NextRequest) {
  if (!adminFromRequest(request)) return jsonError("Unauthorized", 401);
  try {
    const result = await db().query(`
      SELECT g.id, g.name, g.slug, g.allowed_ips, g.created_at,
             count(DISTINCT s.name)::int AS secret_count,
             count(DISTINCT t.id)::int AS token_count
      FROM groups g
      LEFT JOIN secrets s ON s.group_id = g.id
      LEFT JOIN access_tokens t ON t.group_id = g.id
      GROUP BY g.id ORDER BY g.name
    `);
    return NextResponse.json({ groups: result.rows }, { headers: { "Cache-Control": "no-store" } });
  } catch (error) { return serverError(error); }
}

export async function POST(request: NextRequest) {
  if (!adminFromRequest(request)) return jsonError("Unauthorized", 401);
  if (!sameOrigin(request)) return jsonError("Invalid origin", 403);
  const body = await jsonBody(request) as { name?: unknown } | null;
  const name = groupName(body?.name);
  if (!name) return jsonError("Group name must be 1 to 80 characters", 400);
  const slug = groupSlug(name);
  if (!slug) return jsonError("Name must contain a letter or number", 400);
  try {
    const result = await db().query(
      "INSERT INTO groups (name, slug) VALUES ($1, $2) RETURNING id, name, slug, allowed_ips, created_at",
      [name, slug]
    );
    return NextResponse.json({ group: result.rows[0] }, { status: 201 });
  } catch (error) {
    if ((error as { code?: string }).code === "23505") return jsonError("A group with that name already exists", 409);
    return serverError(error);
  }
}
