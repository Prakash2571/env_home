import { createHmac } from "node:crypto";
import { cookies } from "next/headers";
import type { NextRequest } from "next/server";
import { safeEqual } from "@/lib/crypto";

const COOKIE = "env_home_session";
const SESSION_SECONDS = 60 * 60 * 24 * 7;

function secret(): string {
  const value = process.env.AUTH_SECRET;
  if (!value || value.length < 32) throw new Error("AUTH_SECRET must be at least 32 characters");
  return value;
}

function signature(expires: string): string {
  return createHmac("sha256", secret()).update(expires).digest("hex");
}

export function checkPassword(value: string): boolean {
  const expected = process.env.ADMIN_PASSWORD;
  if (!expected || expected.length < 16) throw new Error("ADMIN_PASSWORD must be at least 16 characters");
  return safeEqual(value, expected);
}

export function sessionCookie() {
  const expires = String(Math.floor(Date.now() / 1000) + SESSION_SECONDS);
  return {
    name: COOKIE,
    value: `${expires}.${signature(expires)}`,
    options: {
      httpOnly: true,
      secure: process.env.NODE_ENV === "production",
      sameSite: "strict" as const,
      path: "/",
      maxAge: SESSION_SECONDS,
    },
  };
}

export function validSession(value?: string): boolean {
  if (!value) return false;
  const [expires, mac, extra] = value.split(".");
  if (extra || !expires || !mac || !/^\d+$/.test(expires)) return false;
  const expiry = Number(expires);
  if (!Number.isSafeInteger(expiry) || expiry < Date.now() / 1000) return false;
  if (expiry > Date.now() / 1000 + SESSION_SECONDS) return false;
  return safeEqual(mac, signature(expires));
}

export async function adminFromPage(): Promise<boolean> {
  return validSession((await cookies()).get(COOKIE)?.value);
}

export function adminFromRequest(request: NextRequest): boolean {
  return validSession(request.cookies.get(COOKIE)?.value);
}

export function sameOrigin(request: NextRequest): boolean {
  const origin = request.headers.get("origin");
  return !!origin && origin === new URL(request.url).origin;
}

export const sessionName = COOKIE;
