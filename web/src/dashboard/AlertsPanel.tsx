import { useState } from 'react'
import { ChevronDown, Info, ShieldCheck, TriangleAlert } from 'lucide-react'
import type { AlertsResponse, WeatherAlert } from '../types'
import type { Loadable } from '../store/weather'
import { SEVERITY_STYLE } from '../lib/conditions'
import { dateTime } from '../lib/format'
import { Card, ErrorNote, Skeleton } from '../components/ui'

export function AlertsPanel({ alerts, timeZone }: { alerts: Loadable<AlertsResponse>; timeZone?: string }) {
  const a = alerts.data
  const count = a?.alerts.length ?? 0

  return (
    <Card title={count > 0 ? `Active alerts (${count})` : 'Alerts'} icon={TriangleAlert}>
      {!a && alerts.loading && <Skeleton className="h-12 w-full" />}
      {!a && alerts.error && <ErrorNote message={alerts.error.message} />}
      {a && !a.coverage && (
        <p className="flex items-start gap-2 text-sm text-slate-500 dark:text-slate-400">
          <Info className="mt-0.5 size-4 shrink-0" aria-hidden />
          Severe weather alerts are only available for US locations (National Weather Service).
        </p>
      )}
      {a && a.coverage && count === 0 && (
        <p className="flex items-center gap-2 text-sm text-emerald-700 dark:text-emerald-400">
          <ShieldCheck className="size-4" aria-hidden />
          No active watches, warnings or advisories.
        </p>
      )}
      {count > 0 && (
        <ul className="space-y-2">
          {a!.alerts.map((alert) => (
            <AlertItem key={alert.id} alert={alert} timeZone={timeZone} />
          ))}
        </ul>
      )}
    </Card>
  )
}

function AlertItem({ alert, timeZone }: { alert: WeatherAlert; timeZone?: string }) {
  const [open, setOpen] = useState(false)
  const style = SEVERITY_STYLE[alert.severity]
  return (
    <li className={`rounded-xl border ${style.border}`}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-start gap-3 p-3 text-left"
        aria-expanded={open}
      >
        <span className={`mt-0.5 shrink-0 rounded-md px-1.5 py-0.5 text-[11px] font-semibold uppercase ${style.chip}`}>
          {style.label}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block font-semibold">{alert.event}</span>
          <span className="block text-xs text-slate-500 dark:text-slate-400">
            {alert.expires ? `Until ${dateTime(alert.expires, timeZone)}` : `Since ${dateTime(alert.effective, timeZone)}`}
            {' · '}
            {alert.areaDesc}
          </span>
        </span>
        <ChevronDown className={`mt-1 size-4 shrink-0 text-slate-400 transition ${open ? 'rotate-180' : ''}`} aria-hidden />
      </button>
      {open && (
        <div className="space-y-3 border-t border-slate-100 px-3 pt-3 pb-3 text-sm dark:border-slate-800">
          <p className="font-medium">{alert.headline}</p>
          <p className="whitespace-pre-line text-slate-700 dark:text-slate-300">{alert.description}</p>
          {alert.instruction && (
            <div className="rounded-lg bg-amber-50 p-3 text-amber-950 dark:bg-amber-950/40 dark:text-amber-100">
              <p className="mb-1 text-xs font-semibold uppercase">What to do</p>
              <p className="whitespace-pre-line">{alert.instruction}</p>
            </div>
          )}
        </div>
      )}
    </li>
  )
}
