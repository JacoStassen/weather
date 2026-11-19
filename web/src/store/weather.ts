import { create } from 'zustand'
import { api, ApiError, type Point } from '../api/client'
import type { AlertsResponse, CurrentConditions, Forecast, MarineResponse } from '../types'

export interface Loadable<T> {
  loading: boolean
  data?: T
  error?: { code: string; message: string }
}

const empty = { loading: false }

interface WeatherState {
  /** Identifies what is loaded: location + provider. */
  key: string | null
  current: Loadable<CurrentConditions>
  forecast: Loadable<Forecast>
  alerts: Loadable<AlertsResponse>
  marine: Loadable<MarineResponse>
  loadedAt: number | null
  load: (point: Point, providerId: string) => Promise<void>
  clear: () => void
}

let controller: AbortController | undefined

export const useWeather = create<WeatherState>()((set, get) => ({
  key: null,
  current: empty,
  forecast: empty,
  alerts: empty,
  marine: empty,
  loadedAt: null,

  load: async (point, providerId) => {
    const key = `${point.lat},${point.lon},${providerId}`
    // Keep showing the old data while refreshing the same place; reset otherwise.
    const same = get().key === key
    controller?.abort()
    controller = new AbortController()
    const { signal } = controller

    const start = <T>(prev: Loadable<T>): Loadable<T> => ({ loading: true, data: same ? prev.data : undefined })
    set((s) => ({
      key,
      current: start(s.current),
      forecast: start(s.forecast),
      alerts: start(s.alerts),
      marine: start(s.marine),
    }))

    const run = async <T>(field: 'current' | 'forecast' | 'alerts' | 'marine', fetcher: () => Promise<T>) => {
      try {
        const data = await fetcher()
        if (!signal.aborted) set({ [field]: { loading: false, data } } as Partial<WeatherState>)
      } catch (err) {
        if (signal.aborted) return
        const error =
          err instanceof ApiError
            ? { code: err.code, message: err.message }
            : { code: 'unknown', message: err instanceof Error ? err.message : String(err) }
        set((s) => ({ [field]: { loading: false, data: s[field].data, error } }) as Partial<WeatherState>)
      }
    }

    await Promise.all([
      run('current', () => api.current(point, providerId, signal)),
      run('forecast', () => api.forecast(point, providerId, signal)),
      run('alerts', () => api.alerts(point, signal)),
      run('marine', () => api.marine(point, signal)),
    ])
    if (!signal.aborted) set({ loadedAt: Date.now() })
  },

  clear: () => {
    controller?.abort()
    set({ key: null, current: empty, forecast: empty, alerts: empty, marine: empty, loadedAt: null })
  },
}))
