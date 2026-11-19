import type { CustomProviderConfig, Place } from '../types'
import { getDb } from './client'

export interface SavedLocation {
  id: string
  /** 'device' is the single "my location" entry; it moves with the device. */
  kind: 'saved' | 'device'
  name: string
  region: string | null
  country: string | null
  lat: number
  lon: number
  timezone: string | null
}

/** API key without its secret, for display. */
export interface ApiKeyMeta {
  id: string
  label: string
  /** Last 4 characters, for recognising the key. */
  hint: string
  createdAt: string
}

/** A custom provider as stored: the key is a reference, never inlined. */
export interface StoredCustomProvider {
  config: Omit<CustomProviderConfig, 'apiKey'>
  apiKeyId: string | null
}

const now = () => new Date().toISOString()

// ---- settings --------------------------------------------------------------

export async function getSetting<T>(key: string): Promise<T | undefined> {
  const db = await getDb()
  const [row] = await db.query<{ value: string }>('SELECT value FROM settings WHERE key = ?', [key])
  return row ? (JSON.parse(row.value) as T) : undefined
}

export async function setSetting(key: string, value: unknown): Promise<void> {
  const db = await getDb()
  await db.query(
    'INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value',
    [key, JSON.stringify(value)],
  )
}

export async function deleteSetting(key: string): Promise<void> {
  const db = await getDb()
  await db.query('DELETE FROM settings WHERE key = ?', [key])
}

// ---- locations -------------------------------------------------------------

export async function listLocations(): Promise<SavedLocation[]> {
  const db = await getDb()
  return db.query<SavedLocation>(
    `SELECT id, kind, name, region, country, lat, lon, timezone FROM locations
     ORDER BY kind = 'device' DESC, created_at`,
  )
}

export async function addLocation(place: Place): Promise<SavedLocation> {
  const db = await getDb()
  const loc: SavedLocation = {
    id: crypto.randomUUID(),
    kind: 'saved',
    name: place.name,
    region: place.region,
    country: place.country,
    lat: place.lat,
    lon: place.lon,
    timezone: place.timezone,
  }
  await db.query(
    'INSERT INTO locations (id, name, region, country, lat, lon, timezone, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)',
    [loc.id, loc.name, loc.region, loc.country, loc.lat, loc.lon, loc.timezone, now()],
  )
  return loc
}

/** Creates or moves the device location entry. */
export async function upsertDeviceLocation(
  p: Pick<SavedLocation, 'name' | 'region' | 'country' | 'lat' | 'lon'>,
): Promise<string> {
  const db = await getDb()
  const [existing] = await db.query<{ id: string }>(`SELECT id FROM locations WHERE kind = 'device'`)
  if (existing) {
    await db.query('UPDATE locations SET name = ?, region = ?, country = ?, lat = ?, lon = ?, timezone = NULL WHERE id = ?', [
      p.name,
      p.region,
      p.country,
      p.lat,
      p.lon,
      existing.id,
    ])
    return existing.id
  }
  const id = crypto.randomUUID()
  await db.query(
    `INSERT INTO locations (id, kind, name, region, country, lat, lon, timezone, created_at)
     VALUES (?, 'device', ?, ?, ?, ?, ?, NULL, ?)`,
    [id, p.name, p.region, p.country, p.lat, p.lon, now()],
  )
  return id
}

export async function deleteLocation(id: string): Promise<void> {
  const db = await getDb()
  await db.query('DELETE FROM locations WHERE id = ?', [id])
}

// ---- API keys --------------------------------------------------------------

export async function listApiKeys(): Promise<ApiKeyMeta[]> {
  const db = await getDb()
  return db.query<ApiKeyMeta>(
    `SELECT id, label, substr(secret, -4) AS hint, created_at AS createdAt FROM api_keys ORDER BY created_at`,
  )
}

export async function addApiKey(label: string, secret: string): Promise<void> {
  const db = await getDb()
  await db.query('INSERT INTO api_keys (id, label, secret, created_at) VALUES (?, ?, ?, ?)', [
    crypto.randomUUID(),
    label.trim(),
    secret.trim(),
    now(),
  ])
}

export async function deleteApiKey(id: string): Promise<void> {
  const db = await getDb()
  await db.query('DELETE FROM api_keys WHERE id = ?', [id])
}

async function secrets(): Promise<Map<string, string>> {
  const db = await getDb()
  const rows = await db.query<{ id: string; secret: string }>('SELECT id, secret FROM api_keys')
  return new Map(rows.map((r) => [r.id, r.secret]))
}

// ---- custom providers ------------------------------------------------------

export async function listCustomProviders(): Promise<StoredCustomProvider[]> {
  const db = await getDb()
  const rows = await db.query<{ config: string; api_key_id: string | null }>(
    'SELECT config, api_key_id FROM custom_providers ORDER BY updated_at',
  )
  return rows.map((r) => ({ config: JSON.parse(r.config), apiKeyId: r.api_key_id }))
}

/** Inserts or updates. `previousSlug` handles renames. */
export async function saveCustomProvider(p: StoredCustomProvider, previousSlug?: string): Promise<void> {
  const db = await getDb()
  const { config, apiKeyId } = p
  await db.batch([
    ...(previousSlug && previousSlug !== config.slug
      ? [{ sql: 'DELETE FROM custom_providers WHERE slug = ?', bind: [previousSlug] }]
      : []),
    {
      sql: `INSERT INTO custom_providers (slug, config, api_key_id, updated_at) VALUES (?, ?, ?, ?)
            ON CONFLICT(slug) DO UPDATE SET config = excluded.config, api_key_id = excluded.api_key_id,
            updated_at = excluded.updated_at`,
      bind: [config.slug, JSON.stringify(config), apiKeyId, now()],
    },
  ])
}

export async function deleteCustomProvider(slug: string): Promise<void> {
  const db = await getDb()
  await db.query('DELETE FROM custom_providers WHERE slug = ?', [slug])
}

/** Fills in each provider's key, ready to send to the server. */
export async function resolveCustomProviders(
  stored: StoredCustomProvider[],
): Promise<CustomProviderConfig[]> {
  const keys = await secrets()
  return stored.map(({ config, apiKeyId }) => ({ ...config, apiKey: (apiKeyId && keys.get(apiKeyId)) || null }))
}

/** The secret itself, only for test requests from the settings screen. */
export async function getApiKeySecret(id: string): Promise<string | null> {
  const db = await getDb()
  const [row] = await db.query<{ secret: string }>('SELECT secret FROM api_keys WHERE id = ?', [id])
  return row?.secret ?? null
}
