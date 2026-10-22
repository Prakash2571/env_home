import { Pool } from "pg";

const globalDb = globalThis as typeof globalThis & { envHomePool?: Pool };

export function db(): Pool {
  if (!process.env.DATABASE_URL) throw new Error("DATABASE_URL is required");
  if (!globalDb.envHomePool) {
    globalDb.envHomePool = new Pool({
      connectionString: process.env.DATABASE_URL,
      max: 10,
    });
  }
  return globalDb.envHomePool;
}
