import {
  Cloud,
  CloudDrizzle,
  CloudFog,
  CloudHail,
  CloudLightning,
  CloudMoon,
  CloudRain,
  CloudSnow,
  CloudSun,
  HelpCircle,
  Moon,
  Sun,
  type LucideIcon,
} from 'lucide-react'
import type { Condition, Severity } from '../types'

const LABELS: Record<Condition, string> = {
  clear: 'Clear',
  partly_cloudy: 'Partly cloudy',
  cloudy: 'Cloudy',
  fog: 'Fog',
  drizzle: 'Drizzle',
  rain: 'Rain',
  freezing_rain: 'Freezing rain',
  snow: 'Snow',
  sleet: 'Sleet',
  thunderstorm: 'Thunderstorm',
  unknown: 'Unknown',
}

export const CONDITIONS = Object.keys(LABELS) as Condition[]

export function conditionLabel(c: Condition): string {
  return LABELS[c]
}

export function conditionIcon(c: Condition, isDay = true): LucideIcon {
  switch (c) {
    case 'clear':
      return isDay ? Sun : Moon
    case 'partly_cloudy':
      return isDay ? CloudSun : CloudMoon
    case 'cloudy':
      return Cloud
    case 'fog':
      return CloudFog
    case 'drizzle':
      return CloudDrizzle
    case 'rain':
      return CloudRain
    case 'freezing_rain':
    case 'sleet':
      return CloudHail
    case 'snow':
      return CloudSnow
    case 'thunderstorm':
      return CloudLightning
    default:
      return HelpCircle
  }
}

/** Tailwind classes per alert severity. */
export const SEVERITY_STYLE: Record<Severity, { chip: string; border: string; label: string }> = {
  extreme: {
    chip: 'bg-red-600 text-white',
    border: 'border-red-500/60',
    label: 'Extreme',
  },
  severe: {
    chip: 'bg-orange-500 text-white',
    border: 'border-orange-500/60',
    label: 'Severe',
  },
  moderate: {
    chip: 'bg-amber-400 text-amber-950',
    border: 'border-amber-400/60',
    label: 'Moderate',
  },
  minor: {
    chip: 'bg-sky-200 text-sky-900 dark:bg-sky-900 dark:text-sky-100',
    border: 'border-sky-300/60 dark:border-sky-700/60',
    label: 'Minor',
  },
  unknown: {
    chip: 'bg-slate-200 text-slate-700 dark:bg-slate-700 dark:text-slate-100',
    border: 'border-slate-300 dark:border-slate-700',
    label: 'Advisory',
  },
}
