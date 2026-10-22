import { NextRequest, NextResponse } from "next/server";
import { sameOrigin, sessionName } from "@/lib/auth";
import { jsonError } from "@/lib/http";

export async function POST(request: NextRequest) {
  if (!sameOrigin(request)) return jsonError("Invalid origin", 403);
  const response = NextResponse.json({ ok: true });
  response.cookies.set(sessionName, "", { path: "/", maxAge: 0, httpOnly: true, sameSite: "strict" });
  return response;
}
