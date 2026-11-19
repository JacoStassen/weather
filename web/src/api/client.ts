import type {
  AlertsResponse,
  CurrentConditions,
  CustomProviderConfig,
  CustomProviderTestResult,
  Forecast,
  MarineResponse,
  Place,
  ProviderInfo,
} from '../types'

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message)
  }
}

/**
 * The server's owner token (only set on a server reachable from the
 * internet). Sent with every request: it unlocks custom sources.
 */
let ownerToken: string | undefined

export function setOwnerToken(token: string | undefined) {
  ownerToken = token || undefined
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let res: Response
  try {
    res = await fetch(`/api${path}`, {
      ...init,
      headers: {
        Accept: 'application/json',
        ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
        ...(ownerToken ? { Authorization: `Bearer ${ownerToken}` } : {}),
        ...init?.headers,
      },
    })
  } catch {
    throw new ApiError(0, 'offline', 'Cannot reach the weather server')
  }
  if (!res.ok) {
    const body = await res.json().catch(() => ({}))
    throw new ApiError(res.status, body.code ?? 'http_error', body.message ?? res.statusText)
  }
  return res.json() as Promise<T>
}

function qs(params: Record<string, string | number | null | undefined>): string {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v != null && v !== '') p.set(k, String(v))
  return p.toString()
}

export interface Health {
  status: string
  version: string
  /** Commit the server was built from (release builds only). */
  build: string | null
  /** The server has an owner token: custom sources and MCP need it. */
  ownerTokenRequired: boolean
  /** The token sent with this request (if any) was accepted. */
  owner: boolean
}

export interface Point {
  lat: number
  lon: number
}

export const api = {
  health: () => request<Health>('/health'),
  providers: () => request<ProviderInfo[]>('/providers'),

  current: (p: Point, provider?: string, signal?: AbortSignal) =>
    request<CurrentConditions>(`/weather/current?${qs({ ...p, provider })}`, { signal }),
  forecast: (p: Point, provider?: string, signal?: AbortSignal) =>
    request<Forecast>(`/weather/forecast?${qs({ ...p, provider })}`, { signal }),
  alerts: (p: Point, signal?: AbortSignal) => request<AlertsResponse>(`/alerts?${qs({ ...p })}`, { signal }),
  marine: (p: Point, signal?: AbortSignal) => request<MarineResponse>(`/marine?${qs({ ...p })}`, { signal }),

  geocode: (q: string, signal?: AbortSignal) => request<Place[]>(`/geocode?${qs({ q })}`, { signal }),
  reverseGeocode: (p: Point) => request<Place | null>(`/geocode/reverse?${qs({ ...p })}`),

  replaceCustomProviders: (configs: CustomProviderConfig[]) =>
    request<ProviderInfo[]>('/custom-providers', { method: 'PUT', body: JSON.stringify(configs) }),
  testCustomProvider: (config: CustomProviderConfig, p: Point) =>
    request<CustomProviderTestResult>('/custom-providers/test', {
      method: 'POST',
      body: JSON.stringify({ config, ...p }),
    }),
}
