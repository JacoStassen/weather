import { Sunrise, Sunset, Umbrella } from 'lucide-react'
import type { Forecast } from '../types'
import type { Loadable } from '../store/weather'
import type { Units } from '../store/settings'
import { clock, precip } from '../lib/format'
import { Card, Skeleton } from '../components/ui'

export function TodayCard({ forecast, units }: { forecast: Loadable<Forecast>; units: Units }) {
  const f = forecast.data
  const today = f?.daily[0]
  if (!f && !forecast.loading) return null

  const daylight =
    today?.sunrise && today.sunset ? (Date.parse(today.sunset) - Date.parse(today.sunrise)) / 60000 : null

  return (
    <Card title="Today">
      {!today ? (
        <Skeleton className="h-20 w-full" />
      ) : (
        <dl className="grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
          <Row icon={<Sunrise className="size-4" />} label="Sunrise">
            {today.sunrise ? clock(today.sunrise, f!.timezone) : '–'}
          </Row>
          <Row icon={<Sunset className="size-4" />} label="Sunset">
            {today.sunset ? clock(today.sunset, f!.timezone) : '–'}
          </Row>
          <Row icon={<Umbrella className="size-4" />} label="Rain chance">
            {today.precipProbPct != null ? `${Math.round(today.precipProbPct)}%` : '–'}
          </Row>
          <Row icon={<Umbrella className="size-4" />} label="Total">
            {precip(today.precipMm, units)}
          </Row>
          {daylight != null && (
            <p className="col-span-2 text-xs text-slate-500 dark:text-slate-400">
              {Math.floor(daylight / 60)} h {Math.round(daylight % 60)} min of daylight
            </p>
          )}
        </dl>
      )}
    </Card>
  )
}

function Row({ icon, label, children }: { icon: React.ReactNode; label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt className="flex items-center gap-1.5 text-xs text-slate-500 dark:text-slate-400">
        <span aria-hidden>{icon}</span>
        {label}
      </dt>
      <dd className="mt-0.5 font-medium">{children}</dd>
    </div>
  )
}
