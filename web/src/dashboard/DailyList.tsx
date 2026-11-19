import { CalendarDays, Droplet } from 'lucide-react'
import type { Forecast } from '../types'
import type { Loadable } from '../store/weather'
import type { Units } from '../store/settings'
import { conditionIcon, conditionLabel } from '../lib/conditions'
import { temp, weekday } from '../lib/format'
import { Card, ErrorNote, Skeleton } from '../components/ui'

export function DailyList({ forecast, units }: { forecast: Loadable<Forecast>; units: Units }) {
  const f = forecast.data
  const days = f?.daily ?? []
  const lo = Math.min(...days.map((d) => d.minC))
  const hi = Math.max(...days.map((d) => d.maxC))
  const span = Math.max(hi - lo, 1)

  return (
    <Card title={`${days.length || 7}-day forecast`} icon={CalendarDays}>
      {!f && forecast.loading && <Skeleton className="h-64 w-full" />}
      {!f && forecast.error && <ErrorNote message={forecast.error.message} />}
      {f && (
        <ul className="divide-y divide-slate-100 dark:divide-slate-800">
          {days.map((d, i) => {
            const Icon = conditionIcon(d.condition)
            return (
              <li key={d.date} className="grid grid-cols-[3.5rem_2rem_2.75rem_2.5rem_1fr_2.5rem] items-center gap-2 py-2.5 text-sm">
                <span className="font-medium">{i === 0 ? 'Today' : weekday(d.date)}</span>
                <Icon className="size-5 text-slate-600 dark:text-slate-300" aria-label={conditionLabel(d.condition)} />
                <span className="flex items-center gap-0.5 text-xs font-medium text-sky-600 dark:text-sky-400">
                  {(d.precipProbPct ?? 0) >= 10 && (
                    <>
                      <Droplet className="size-3" aria-hidden />
                      {Math.round(d.precipProbPct!)}%
                    </>
                  )}
                </span>
                <span className="text-right text-slate-500 dark:text-slate-400">{temp(d.minC, units)}</span>
                <span className="relative h-1.5 rounded-full bg-slate-100 dark:bg-slate-800" aria-hidden>
                  <span
                    className="absolute inset-y-0 rounded-full bg-gradient-to-r from-sky-400 to-amber-400"
                    style={{ left: `${((d.minC - lo) / span) * 100}%`, right: `${((hi - d.maxC) / span) * 100}%` }}
                  />
                </span>
                <span className="font-semibold">{temp(d.maxC, units)}</span>
              </li>
            )
          })}
        </ul>
      )}
    </Card>
  )
}
