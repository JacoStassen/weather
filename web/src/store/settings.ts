import { create } from 'zustand'
import { api } from '../api/client'
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
  selectedLocationId: string | null
  locations: SavedLocation[]
  apiKeys: ApiKeyMeta[]
  customProviders: StoredCustomProvider[]

  /** Providers the server currently knows, including synced custom ones. */
  serverProviders: ProviderInfo[]
  syncError?: string

  locating: boolean
  locateError?: string

  /** Asks for the device position (may prompt) and selects it. */
  locateDevice: () => Promise<void>
  /** Silently updates the device location if permission was already granted. */
  refreshDeviceLocation: () => Promise<void>

  init: () => Promise<void>
  setActiveProvider: (id: string) => Promise<void>
  setUnits: (u: Units) => Promise<void>
  selectLocation: (id: string) => Promise<void>
  addLocation: (place: Place) => Promise<void>
  removeLocation: (id: string) => Promise<void>
  addApiKey: (label: string, secret: string) => Promise<void>
  removeApiKey: (id: string) => Promise<void>
  saveCustomProvider: (p: StoredCustomProvider, previousSlug?: string) => Promise<void>
  removeCustomProvider: (slug: string) => Promise<void>
  syncProviders: (opts?: { startup?: boolean }) => Promise<void>
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
    selectedLocationId: null,
    locations: [],
    apiKeys: [],
    customProviders: [],
    serverProviders: [],
    locating: false,

    init: async () => {
      try {
        const db = await getDb()
        const [activeProviderId, units, selectedLocationId] = await Promise.all([
          repo.getSetting<string>('activeProviderId'),
          repo.getSetting<Units>('units'),
          repo.getSetting<string>('selectedLocationId'),
        ])
        await reload()
        const { locations } = get()
        set({
          status: 'ready',
          persistent: db.persistent,
          activeProviderId: activeProviderId ?? DEFAULT_PROVIDER,
          units: units ?? (navigator.language === 'en-US' ? 'imperial' : 'metric'),
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
        const push = get().persistent && !(startup && get().customProviders.length === 0)
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
  }
})

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
