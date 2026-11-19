import { Clock, Droplet } from 'lucide-react'
import type { Forecast } from '../types'
import type { Loadable } from '../store/weather'
import type { Units } from '../store/settings'
import { conditionIcon, conditionLabel } from '../lib/conditions'
import { hourLabel, temp } from '../lib/format'
import { Card, ErrorNote, Skeleton } from '../components/ui'

export function HourlyStrip({ forecast, units }: { forecast: Loadable<Forecast>; units: Units }) {
  const f = forecast.data
  const now = Date.now()
  // From the current hour, 24 hours ahead.
  const hours = f?.hourly.filter((h) => Date.parse(h.time) > now - 3600_000).slice(0, 24) ?? []

  return (
    <Card title="Next 24 hours" icon={Clock}>
      {!f && forecast.loading && <Skeleton className="h-24 w-full" />}
      {!f && forecast.error && <ErrorNote message={forecast.error.message} />}
      {f && (
        <ol className="-mx-1 flex snap-x gap-1 overflow-x-auto pb-1" aria-label="Hourly forecast">
          {hours.map((h, i) => {
            const Icon = conditionIcon(h.condition, h.isDay)
            return (
              <li key={h.time} className="flex w-16 shrink-0 snap-start flex-col items-center gap-1.5 rounded-xl px-1 py-2">
                <span className="text-xs text-slate-500 dark:text-slate-400">
                  {i === 0 ? 'Now' : hourLabel(h.time, f.timezone)}
                </span>
                <Icon className="size-6 text-slate-600 dark:text-slate-300" aria-label={conditionLabel(h.condition)} />
                <span className="text-sm font-semibold">{temp(h.temperatureC, units)}</span>
                <span
                  className={`flex items-center gap-0.5 text-[11px] font-medium text-sky-600 dark:text-sky-400 ${
                    (h.precipProbPct ?? 0) >= 10 ? '' : 'invisible'
                  }`}
                >
                  <Droplet className="size-3" aria-hidden />
                  {Math.round(h.precipProbPct ?? 0)}%
                </span>
              </li>
            )
          })}
        </ol>
      )}
    </Card>
  )
}
