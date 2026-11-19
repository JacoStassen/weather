import type { Units } from '../store/settings'

export function temp(c: number | null | undefined, units: Units): string {
  if (c == null) return '–'
  const v = units === 'imperial' ? (c * 9) / 5 + 32 : c
  return `${Math.round(v)}°`
}

export function tempUnit(units: Units): string {
  return units === 'imperial' ? '°F' : '°C'
}

export function wind(ms: number | null | undefined, units: Units): string {
  if (ms == null) return '–'
  return units === 'imperial' ? `${Math.round(ms * 2.23694)} mph` : `${Math.round(ms * 3.6)} km/h`
}

export function precip(mm: number | null | undefined, units: Units): string {
  if (mm == null) return '–'
  return units === 'imperial' ? `${(mm / 25.4).toFixed(2)} in` : `${mm.toFixed(1)} mm`
}

export function pressure(hpa: number | null | undefined, units: Units): string {
  if (hpa == null) return '–'
  return units === 'imperial' ? `${(hpa / 33.8639).toFixed(2)} inHg` : `${Math.round(hpa)} hPa`
}

export function length(m: number | null | undefined, units: Units, digits = 1): string {
  if (m == null) return '–'
  const v = units === 'imperial' ? m * 3.28084 : m
  // Avoid "-0.00".
  const s = (Math.abs(v) < 0.5 * 10 ** -digits ? 0 : v).toFixed(digits)
  return units === 'imperial' ? `${s} ft` : `${s} m`
}

export function pct(v: number | null | undefined): string {
  return v == null ? '–' : `${Math.round(v)}%`
}

const COMPASS = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW']
export function compass(deg: number | null | undefined): string {
  if (deg == null) return ''
  return COMPASS[Math.round((((deg % 360) + 360) % 360) / 45) % 8]
}

/** Clock time at the location (not the viewer's timezone). */
export function clock(iso: string, timeZone?: string): string {
  return new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit', timeZone }).format(new Date(iso))
}

export function hourLabel(iso: string, timeZone?: string): string {
  return new Intl.DateTimeFormat(undefined, { hour: 'numeric', timeZone }).format(new Date(iso))
}

/** Weekday for a plain `YYYY-MM-DD` date. */
export function weekday(date: string, style: 'short' | 'long' = 'short'): string {
  return new Intl.DateTimeFormat(undefined, { weekday: style, timeZone: 'UTC' }).format(new Date(`${date}T12:00:00Z`))
}

export function dateTime(iso: string, timeZone?: string): string {
  return new Intl.DateTimeFormat(undefined, {
    weekday: 'short',
    hour: 'numeric',
    minute: '2-digit',
    timeZone,
  }).format(new Date(iso))
}

export function ago(iso: string | number, now = Date.now()): string {
  const t = typeof iso === 'number' ? iso : Date.parse(iso)
  const mins = Math.round((now - t) / 60000)
  if (mins < 1) return 'just now'
  if (mins < 60) return `${mins} min ago`
  const hours = Math.round(mins / 60)
  if (hours < 24) return `${hours} h ago`
  return `${Math.round(hours / 24)} d ago`
}

/** "Name, Region, Country" without empty parts or repeats. */
export function placeLabel(p: { name: string; region?: string | null; country?: string | null }): string {
  return [p.name, p.region, p.country].filter((v, i, a) => v && a.indexOf(v) === i).join(', ')
}
