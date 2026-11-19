import type { Db } from './client'

/** Append only. Each entry moves `PRAGMA user_version` up by one. */
const MIGRATIONS: string[][] = [
  [
    `CREATE TABLE settings (
       key   TEXT PRIMARY KEY,
       value TEXT NOT NULL            -- JSON
     )`,
    `CREATE TABLE api_keys (
       id         TEXT PRIMARY KEY,
       label      TEXT NOT NULL,
       secret     TEXT NOT NULL,
       created_at TEXT NOT NULL
     )`,
    `CREATE TABLE custom_providers (
       slug       TEXT PRIMARY KEY,
       config     TEXT NOT NULL,      -- CustomProviderConfig JSON, without the key
       api_key_id TEXT REFERENCES api_keys(id) ON DELETE SET NULL,
       updated_at TEXT NOT NULL
     )`,
    `CREATE TABLE locations (
       id         TEXT PRIMARY KEY,
       name       TEXT NOT NULL,
       region     TEXT,
       country    TEXT,
       lat        REAL NOT NULL,
       lon        REAL NOT NULL,
       timezone   TEXT,
       created_at TEXT NOT NULL
     )`,
  ],
  [
    // 'device' marks the "my location" entry, whose coordinates follow the device.
    `ALTER TABLE locations ADD COLUMN kind TEXT NOT NULL DEFAULT 'saved'`,
  ],
]

export async function migrate(db: Db): Promise<void> {
  const [{ user_version }] = await db.query<{ user_version: number }>('PRAGMA user_version')
  for (let v = user_version; v < MIGRATIONS.length; v++) {
    await db.batch([...MIGRATIONS[v].map((sql) => ({ sql })), { sql: `PRAGMA user_version = ${v + 1}` }])
  }
}
