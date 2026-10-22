import { NextResponse } from "next/server";

export function jsonError(message: string, status: number) {
  return NextResponse.json({ error: message }, { status, headers: { "Cache-Control": "no-store" } });
}

export function serverError(error: unknown) {
  console.error("Request failed", error);
  return jsonError("Server error", 500);
}

export async function jsonBody(request: Request): Promise<unknown> {
  try { return await request.json(); } catch { return null; }
}
