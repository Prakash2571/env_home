import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { loadEnvFile } from "node:process";
import pg from "pg";

if (existsSync(".env")) loadEnvFile(".env");
if (!process.env.DATABASE_URL) throw new Error("DATABASE_URL is required");

const { Pool } = pg;
const pool = new Pool({ connectionString: process.env.DATABASE_URL });
try {
  const sql = await readFile(new URL("../db/schema.sql", import.meta.url), "utf8");
  await pool.query(sql);
  console.log("Database ready");
} finally {
  await pool.end();
}
