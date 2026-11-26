import type { Condition } from '../types'

/** The look of the dashboard, picked from the current weather. */
export type Mood = 'sunny' | 'partly' | 'night' | 'cloudy' | 'fog' | 'rain' | 'storm' | 'snow'

export function moodFor(condition: Condition, isDay: boolean): Mood {
  switch (condition) {
    case 'clear':
      return isDay ? 'sunny' : 'night'
    case 'partly_cloudy':
      return isDay ? 'partly' : 'night'
    case 'cloudy':
      return 'cloudy'
    case 'fog':
      return 'fog'
    case 'drizzle':
    case 'rain':
    case 'freezing_rain':
    case 'sleet':
      return 'rain'
    case 'thunderstorm':
      return 'storm'
    case 'snow':
      return 'snow'
    default:
      return isDay ? 'partly' : 'night'
  }
}

interface Theme {
  /** Background of the current-conditions card (white text sits on it). */
  hero: string
  /** Tint washed over the top of the page, behind the cards. */
  wash: string
  /** Browser/OS bar colour on phones. */
  bar: string
}

/** The plain look, used when weather themes are off. */
export const PLAIN: Record<'day' | 'night', Theme> = {
  day: { hero: 'from-sky-500 to-blue-700', wash: '', bar: '#0ea5e9' },
  night: { hero: 'from-indigo-950 via-slate-900 to-slate-800', wash: '', bar: '#0f172a' },
}

export const THEMES: Record<Mood, Theme> = {
  sunny: {
    hero: 'from-amber-500 via-orange-500 to-orange-700',
    wash: 'from-amber-200/70 via-orange-100/40 dark:from-amber-500/15 dark:via-orange-500/5',
    bar: '#f59e0b',
  },
  partly: {
    hero: 'from-sky-500 via-sky-600 to-indigo-600',
    wash: 'from-sky-200/70 via-amber-50/40 dark:from-sky-500/15 dark:via-sky-500/5',
    bar: '#0ea5e9',
  },
  night: {
    hero: 'from-indigo-950 via-slate-900 to-violet-950',
    wash: 'from-indigo-200/60 via-violet-100/30 dark:from-indigo-500/15 dark:via-violet-500/5',
    bar: '#1e1b4b',
  },
  cloudy: {
    hero: 'from-slate-500 via-slate-600 to-slate-800',
    wash: 'from-slate-300/70 via-slate-200/40 dark:from-slate-400/15 dark:via-slate-500/5',
    bar: '#64748b',
  },
  fog: {
    hero: 'from-slate-400 via-zinc-500 to-slate-700',
    wash: 'from-zinc-300/70 via-zinc-200/40 dark:from-zinc-400/15 dark:via-zinc-500/5',
    bar: '#71717a',
  },
  rain: {
    hero: 'from-slate-600 via-sky-800 to-indigo-900',
    wash: 'from-sky-300/50 via-slate-200/40 dark:from-sky-700/20 dark:via-slate-500/5',
    bar: '#075985',
  },
  storm: {
    hero: 'from-slate-800 via-indigo-950 to-violet-950',
    wash: 'from-violet-300/50 via-slate-300/40 dark:from-violet-700/20 dark:via-slate-600/10',
    bar: '#2e1065',
  },
  snow: {
    hero: 'from-sky-600 via-slate-500 to-indigo-500',
    wash: 'from-sky-100/80 via-white/40 dark:from-sky-300/10 dark:via-slate-300/5',
    bar: '#7dd3fc',
  },
}
