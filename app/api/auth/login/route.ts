import { NextRequest, NextResponse } from "next/server";
import { checkPassword, sameOrigin, sessionCookie } from "@/lib/auth";
import { jsonBody, jsonError, serverError } from "@/lib/http";

export async function POST(request: NextRequest) {
  if (!sameOrigin(request)) return jsonError("Invalid origin", 403);
  const body = await jsonBody(request) as { password?: unknown } | null;
  if (!body || typeof body.password !== "string") return jsonError("Password required", 400);
  try {
    if (!checkPassword(body.password)) return jsonError("Invalid password", 401);
    const response = NextResponse.json({ ok: true });
    const cookie = sessionCookie();
    response.cookies.set(cookie.name, cookie.value, cookie.options);
    response.headers.set("Cache-Control", "no-store");
    return response;
  } catch (error) { return serverError(error); }
}
