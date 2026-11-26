import { create } from 'zustand'
import { ApiError, api, setOwnerToken as useTokenForRequests } from '../api/client'
import * as repo from '../db/repo'
import { getDb } from '../db/client'
import type { ApiKeyMeta, SavedLocation, StoredCustomProvider } from '../db/repo'
import type { Place, ProviderInfo } from '../types'
import { distanceKm, getPosition, permission } from '../lib/geolocation'

/** Move the device location only when it changed by more than this. */
const DEVICE_MOVE_KM = 1

export type Units = 'metric' | 'imperial'
export const DEFAULT_PROVIDER = 'open-meteo'

interface SettingsState {
  status: 'loading' | 'ready' | 'error'
  error?: string
  /** False when settings live only in memory (no OPFS, or another tab has the database). */
  persistent: boolean

  activeProviderId: string
  units: Units
  /** Colours follow the weather (sunny, rainy…). Off: the plain blue look. */
  weatherThemes: boolean
  /** Moving effects (rain, drifting clouds…). Defaults to off when the device asks for reduced motion. */
  animations: boolean
  selectedLocationId: string | null
  locations: SavedLocation[]
  apiKeys: ApiKeyMeta[]
  customProviders: StoredCustomProvider[]

  /** Providers the server currently knows, including synced custom ones. */
  serverProviders: ProviderInfo[]
  syncError?: string

  /**
   * Access to a server reachable from the internet. `tokenRequired` false
   * (a local server) means everything is open and `owner` is true.
   */
  server: { tokenRequired: boolean; owner: boolean; tokenRejected: boolean } | null
  /** Saved owner token, if any. */
  ownerToken?: string
  /** Checks the token with the server and saves it if it works. Returns an error message. */
  saveOwnerToken: (token: string) => Promise<string | undefined>
  forgetOwnerToken: () => Promise<void>

  locating: boolean
  locateError?: string

  /** Asks for the device position (may prompt) and selects it. */
  locateDevice: () => Promise<void>
  /** Silently updates the device location if permission was already granted. */
  refreshDeviceLocation: () => Promise<void>

  init: () => Promise<void>
  setActiveProvider: (id: string) => Promise<void>
  setUnits: (u: Units) => Promise<void>
  setWeatherThemes: (on: boolean) => Promise<void>
  setAnimations: (on: boolean) => Promise<void>
  selectLocation: (id: string) => Promise<void>
  addLocation: (place: Place) => Promise<void>
  removeLocation: (id: string) => Promise<void>
  addApiKey: (label: string, secret: string) => Promise<void>
  removeApiKey: (id: string) => Promise<void>
  saveCustomProvider: (p: StoredCustomProvider, previousSlug?: string) => Promise<void>
  removeCustomProvider: (slug: string) => Promise<void>
  syncProviders: (opts?: { startup?: boolean }) => Promise<void>
}

function prefersReducedMotion(): boolean {
  return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches
}

export const useSettings = create<SettingsState>()((set, get) => {
  /** Reloads lists from the database after a change. */
  const reload = async () => {
    const [locations, apiKeys, customProviders] = await Promise.all([
      repo.listLocations(),
      repo.listApiKeys(),
      repo.listCustomProviders(),
    ])
    set({ locations, apiKeys, customProviders })
  }

  return {
    status: 'loading',
    persistent: false,
    activeProviderId: DEFAULT_PROVIDER,
    units: 'metric',
    weatherThemes: true,
    animations: !prefersReducedMotion(),
    selectedLocationId: null,
    locations: [],
    apiKeys: [],
    customProviders: [],
    serverProviders: [],
    server: null,
    locating: false,

    init: async () => {
      try {
        const db = await getDb()
        const [activeProviderId, units, selectedLocationId, ownerToken, weatherThemes, animations] = await Promise.all([
          repo.getSetting<string>('activeProviderId'),
          repo.getSetting<Units>('units'),
          repo.getSetting<string>('selectedLocationId'),
          repo.getSetting<string>('ownerToken'),
          repo.getSetting<boolean>('weatherThemes'),
          repo.getSetting<boolean>('animations'),
        ])
        // Before the dashboard's first request: a stale token would get every
        // request refused. Offline is fine, syncProviders reports it.
        const server = await checkServer(ownerToken).catch(() => null)
        await reload()
        const { locations } = get()
        set({
          status: 'ready',
          persistent: db.persistent,
          ownerToken,
          server,
          activeProviderId: activeProviderId ?? DEFAULT_PROVIDER,
          units: units ?? (navigator.language === 'en-US' ? 'imperial' : 'metric'),
          weatherThemes: weatherThemes ?? true,
          animations: animations ?? !prefersReducedMotion(),
          selectedLocationId: locations.some((l) => l.id === selectedLocationId)
            ? selectedLocationId!
            : (locations[0]?.id ?? null),
        })
      } catch (err) {
        set({ status: 'error', error: err instanceof Error ? err.message : String(err) })
        return
      }
      await get().syncProviders({ startup: true })
    },

    setActiveProvider: async (id) => {
      set({ activeProviderId: id })
      await repo.setSetting('activeProviderId', id)
    },

    setUnits: async (units) => {
      set({ units })
      await repo.setSetting('units', units)
    },

    setWeatherThemes: async (weatherThemes) => {
      set({ weatherThemes })
      await repo.setSetting('weatherThemes', weatherThemes)
    },

    setAnimations: async (animations) => {
      set({ animations })
      await repo.setSetting('animations', animations)
    },

    selectLocation: async (id) => {
      set({ selectedLocationId: id })
      await repo.setSetting('selectedLocationId', id)
    },

    addLocation: async (place) => {
      const loc = await repo.addLocation(place)
      await reload()
      await get().selectLocation(loc.id)
    },

    removeLocation: async (id) => {
      await repo.deleteLocation(id)
      await reload()
      if (get().selectedLocationId === id) {
        const next = get().locations[0]?.id ?? null
        set({ selectedLocationId: next })
        await repo.setSetting('selectedLocationId', next)
      }
    },

    addApiKey: async (label, secret) => {
      await repo.addApiKey(label, secret)
      await reload()
    },

    removeApiKey: async (id) => {
      const usedBy = get().customProviders.some((p) => p.apiKeyId === id)
      await repo.deleteApiKey(id)
      await reload()
      if (usedBy) await get().syncProviders()
    },

    saveCustomProvider: async (p, previousSlug) => {
      await repo.saveCustomProvider(p, previousSlug)
      await reload()
      const { activeProviderId } = get()
      if (previousSlug && activeProviderId === `custom:${previousSlug}`) {
        await get().setActiveProvider(`custom:${p.config.slug}`)
      }
      await get().syncProviders()
    },

    removeCustomProvider: async (slug) => {
      await repo.deleteCustomProvider(slug)
      await reload()
      if (get().activeProviderId === `custom:${slug}`) await get().setActiveProvider(DEFAULT_PROVIDER)
      await get().syncProviders()
    },

    locateDevice: async () => {
      set({ locating: true, locateError: undefined })
      try {
        const pos = await getPosition()
        const id = await saveDevicePosition(pos)
        await reload()
        await get().selectLocation(id)
      } catch (err) {
        set({ locateError: err instanceof Error ? err.message : String(err) })
      } finally {
        set({ locating: false })
      }
    },

    refreshDeviceLocation: async () => {
      const device = get().locations.find((l) => l.kind === 'device')
      if (!device || get().locating || (await permission()) !== 'granted') return
      try {
        const pos = await getPosition()
        if (distanceKm(device, pos) < DEVICE_MOVE_KM) return
        await saveDevicePosition(pos)
        await reload()
      } catch {
        // Keep the last known position.
      }
    },

    /**
     * Pushes custom providers to the server, which then serves them to the
     * dashboard and to MCP clients.
     *
     * Never pushes an empty list just because the app started: an in-memory
     * database (second tab) or a lost browser database would otherwise wipe
     * the server's copy too. Deleting the last source pushes explicitly.
     */
    syncProviders: async ({ startup = false } = {}) => {
      try {
        const server = await checkServer(get().ownerToken)
        set({ server })
        // Without the owner token the server won't take custom sources (and
        // its copy belongs to whoever has the token): only read.
        const push = server.owner && get().persistent && !(startup && get().customProviders.length === 0)
        if (push) {
          const configs = await repo.resolveCustomProviders(get().customProviders)
          set({ serverProviders: await api.replaceCustomProviders(configs), syncError: undefined })
        } else {
          set({ serverProviders: await api.providers(), syncError: undefined })
        }
      } catch (err) {
        set({ syncError: err instanceof Error ? err.message : String(err) })
      }
    },

    saveOwnerToken: async (token) => {
      token = token.trim()
      const server = await checkServer(token).catch(() => null)
      if (!server) return 'Couldn’t reach the weather server.'
      if (!server.tokenRequired) return 'This server doesn’t use an access token.'
      if (!server.owner) {
        // Keep using the previous token, if it worked.
        useTokenForRequests(get().server?.owner ? get().ownerToken : undefined)
        return 'The server didn’t accept that token.'
      }
      await repo.setSetting('ownerToken', token)
      set({ ownerToken: token })
      // `startup` semantics: a new device with no sources yet must not wipe
      // the server's copy.
      await get().syncProviders({ startup: get().customProviders.length === 0 })
      return undefined
    },

    forgetOwnerToken: async () => {
      await repo.deleteSetting('ownerToken')
      useTokenForRequests(undefined)
      set({ ownerToken: undefined })
      await get().syncProviders({ startup: true })
    },
  }
})

/**
 * Asks the server what this token allows. A rejected token is dropped from
 * requests, since the server refuses every request that carries a wrong one
 * and the dashboard would stop working.
 */
async function checkServer(token: string | undefined): Promise<NonNullable<SettingsState['server']>> {
  useTokenForRequests(token)
  try {
    const h = await api.health()
    return { tokenRequired: h.ownerTokenRequired, owner: h.owner, tokenRejected: false }
  } catch (err) {
    if (!(err instanceof ApiError && err.status === 401)) throw err
    useTokenForRequests(undefined)
    const h = await api.health()
    return { tokenRequired: h.ownerTokenRequired, owner: h.owner, tokenRejected: true }
  }
}

/** Stores the position, named by reverse geocoding when the server can. */
async function saveDevicePosition(pos: { lat: number; lon: number }): Promise<string> {
  const place = await api.reverseGeocode(pos).catch(() => null)
  return repo.upsertDeviceLocation({
    name: place?.name ?? 'My location',
    region: place?.region ?? null,
    country: place?.country ?? null,
    lat: pos.lat,
    lon: pos.lon,
  })
}

export function useSelectedLocation(): SavedLocation | null {
  return useSettings((s) => s.locations.find((l) => l.id === s.selectedLocationId) ?? null)
}
