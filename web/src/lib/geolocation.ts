export type GeoFailure = 'unsupported' | 'denied' | 'unavailable' | 'timeout'

export class GeoError extends Error {
  constructor(public reason: GeoFailure) {
    super(
      {
        unsupported: 'This browser can’t share your location.',
        denied: 'Location access is blocked. Allow it in your browser’s site settings, or search for a place instead.',
        unavailable: 'Your location isn’t available right now. Try again, or search for a place.',
        timeout: 'Finding your location took too long. Try again, or search for a place.',
      }[reason],
    )
  }
}

export interface Position {
  lat: number
  lon: number
  /** Metres. */
  accuracy: number
}

/** City-level accuracy is plenty for weather, and much faster than GPS. */
export function getPosition(): Promise<Position> {
  if (!('geolocation' in navigator)) return Promise.reject(new GeoError('unsupported'))
  return new Promise((resolve, reject) => {
    navigator.geolocation.getCurrentPosition(
      (p) => resolve({ lat: p.coords.latitude, lon: p.coords.longitude, accuracy: p.coords.accuracy }),
      (e) =>
        reject(
          new GeoError(
            e.code === e.PERMISSION_DENIED ? 'denied' : e.code === e.TIMEOUT ? 'timeout' : 'unavailable',
          ),
        ),
      { enableHighAccuracy: false, timeout: 15_000, maximumAge: 10 * 60_000 },
    )
  })
}

/** 'granted' means we can update the position without prompting. */
export async function permission(): Promise<PermissionState | 'unknown'> {
  try {
    return (await navigator.permissions.query({ name: 'geolocation' })).state
  } catch {
    return 'unknown'
  }
}

/** Great-circle distance in kilometres. */
export function distanceKm(a: { lat: number; lon: number }, b: { lat: number; lon: number }): number {
  const rad = Math.PI / 180
  const dLat = (b.lat - a.lat) * rad
  const dLon = (b.lon - a.lon) * rad
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(a.lat * rad) * Math.cos(b.lat * rad) * Math.sin(dLon / 2) ** 2
  return 2 * 6371 * Math.asin(Math.sqrt(h))
}
