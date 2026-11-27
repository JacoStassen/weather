import { useEffect, useMemo, useState } from 'react'
import { Columns3, Droplet } from 'lucide-react'
import { api } from '../api/client'
import type { CurrentConditions, Forecast, ProviderInfo } from '../types'
import { useSelectedLocation, useSettings, type Units } from '../store/settings'
import { conditionIcon, conditionLabel } from '../lib/conditions'
import { placeLabel, temp, weekday } from '../lib/format'
import { Card, Spinner } from '../components/ui'

const DAYS = 7
const NOT_INDEPENDENT = ['blend', 'open-meteo']

interface Row {
  provider: ProviderInfo
  loading: boolean
  current?: CurrentConditions
  forecast?: Forecast
  error?: string
}

/**
 * The selected place's forecast from every source side by side, so you can see
 * where they agree before picking one.
 */
export function CompareSources() {
  const location = useSelectedLocation()
  const providers = useSettings((s) => s.serverProviders)
  const active = useSettings((s) => s.activeProviderId)
  const setActive = useSettings((s) => s.setActiveProvider)
  const units = useSettings((s) => s.units)

  const sources = useMemo(
    () => providers.filter((p) => p.capabilities.includes('current') || p.capabilities.includes('daily')),
    [providers],
  )
  const [rows, setRows] = useState<Record<string, Row>>({})

  const lat = location?.lat
  const lon = location?.lon
  useEffect(() => {
    if (lat == null || lon == null) return
    const ctl = new AbortController()
    const point = { lat, lon }
    setRows(Object.fromEntries(sources.map((p) => [p.id, { provider: p, loading: true }])))

    for (const p of sources) {
      const update = (patch: Partial<Row>) =>
        !ctl.signal.aborted && setRows((r) => ({ ...r, [p.id]: { ...r[p.id], ...patch } }))
      const daily = p.capabilities.includes('daily')
      Promise.allSettled([
        p.capabilities.includes('current') ? api.current(point, p.id, ctl.signal) : Promise.resolve(undefined),
        daily ? api.forecast(point, p.id, ctl.signal) : Promise.resolve(undefined),
      ]).then(([c, f]) => {
        const failed = [c, f].find((x) => x.status === 'rejected') as PromiseRejectedResult | undefined
        update({
          loading: false,
          current: c.status === 'fulfilled' ? c.value : undefined,
          // A source without its own forecast would be filled in from Open-Meteo; don't show that as its own.
          forecast: f.status === 'fulfilled' && f.value?.source.providerId === p.id ? f.value : undefined,
          error: failed ? String(failed.reason?.message ?? failed.reason) : undefined,
        })
      })
    }
    return () => ctl.abort()
  }, [lat, lon, sources])

  // Columns follow the first source with a forecast (normally Open-Meteo).
  const all = sources.map((p) => rows[p.id]).filter(Boolean)
  const dates = (all.find((r) => r.forecast)?.forecast?.daily ?? []).slice(0, DAYS).map((d) => d.date)

  // How far apart the sources' highs are, per day: a quick read on how sure the forecast is.
  // Only independent forecasts: our blend averages them, and Open-Meteo's mostly repeats ECMWF.
  const independent = all.filter((r) => !NOT_INDEPENDENT.includes(r.provider.id))
  const spread = dates.map((date) => {
    const highs = independent.flatMap((r) => r.forecast?.daily.find((d) => d.date === date)?.maxC ?? [])
    return highs.length > 1 ? Math.max(...highs) - Math.min(...highs) : null
  })

  if (!location) return null

  return (
    <Card title="Compare forecasts" icon={Columns3}>
      <p className="mb-3 text-sm text-slate-500 dark:text-slate-400">
        Every source’s forecast for {placeLabel(location)}. Daily highs and lows; the blue figure is the chance of rain.
      </p>

      <div className="-mx-4 overflow-x-auto sm:-mx-5">
        <table className="w-full min-w-[36rem] border-separate border-spacing-0 text-sm">
          <thead>
            <tr className="text-xs text-slate-500 dark:text-slate-400">
              <th className="sticky left-0 z-10 bg-white py-2 pr-2 pl-4 text-left font-medium sm:pl-5 dark:bg-slate-900">Source</th>
              <th className="px-1 py-2 text-center font-medium">Now</th>
              {dates.map((d, i) => (
                <th key={d} className="px-1 py-2 text-center font-medium">
                  {i === 0 ? 'Today' : weekday(d)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {sources.map((p) => {
              const r = rows[p.id]
              const inUse = p.id === active
              return (
                <tr key={p.id} className={inUse ? 'bg-sky-50 dark:bg-sky-950/40' : ''}>
                  <th
                    scope="row"
                    className={`sticky left-0 z-10 border-t border-slate-100 py-2 pr-2 pl-4 text-left font-normal sm:pl-5 dark:border-slate-800 ${
                      inUse ? 'bg-sky-50 dark:bg-sky-950' : 'bg-white dark:bg-slate-900'
                    }`}
                  >
                    <span className="block max-w-[9rem] truncate font-medium" title={p.name}>
                      {p.name}
                    </span>
                    {inUse ? (
                      <span className="text-xs text-sky-700 dark:text-sky-400">In use</span>
                    ) : (
                      <button
                        type="button"
                        onClick={() => void setActive(p.id)}
                        className="text-xs text-slate-500 underline-offset-2 hover:text-sky-700 hover:underline dark:text-slate-400 dark:hover:text-sky-400"
                      >
                        Use this
                      </button>
                    )}
                  </th>
                  {!r || r.loading ? (
                    <td colSpan={dates.length + 1} className="border-t border-slate-100 px-2 dark:border-slate-800">
                      <Spinner className="size-4" />
                    </td>
                  ) : (
                    <>
                      <td className="border-t border-slate-100 px-1 py-2 text-center dark:border-slate-800">
                        {r.current ? (
                          <span className="font-semibold">{temp(r.current.temperatureC, units)}</span>
                        ) : (
                          <Dash title={r.error} />
                        )}
                      </td>
                      {dates.map((date) => (
                        <td key={date} className="border-t border-slate-100 px-1 py-2 dark:border-slate-800">
                          <DayCell day={r.forecast?.daily.find((d) => d.date === date)} units={units} />
                        </td>
                      ))}
                    </>
                  )}
                </tr>
              )
            })}
            {spread.some((s) => s != null) && (
              <tr className="text-xs text-slate-500 dark:text-slate-400">
                <th
                  scope="row"
                  className="sticky left-0 z-10 border-t border-slate-200 bg-white py-2 pr-2 pl-4 text-left font-normal sm:pl-5 dark:border-slate-700 dark:bg-slate-900"
                  title="Difference between the warmest and coolest high across the sources"
                >
                  Spread of highs
                </th>
                <td className="border-t border-slate-200 dark:border-slate-700" />
                {spread.map((s, i) => (
                  <td key={dates[i]} className="border-t border-slate-200 px-1 py-2 text-center dark:border-slate-700">
                    {s == null ? '–' : <SpreadValue degreesC={s} units={units} />}
                  </td>
                ))}
              </tr>
            )}
          </tbody>
        </table>
      </div>
      <p className="mt-3 text-xs text-slate-500 dark:text-slate-400">
        A small spread means the sources agree; a large one means the forecast is less certain.
      </p>
    </Card>
  )
}

function DayCell({ day, units }: { day?: Forecast['daily'][number]; units: Units }) {
  if (!day) return <Dash />
  const Icon = conditionIcon(day.condition)
  const rain = day.precipProbPct ?? 0
  return (
    <div className="flex flex-col items-center gap-0.5 leading-tight">
      <Icon className="size-4 text-slate-500 dark:text-slate-400" aria-label={conditionLabel(day.condition)} />
      <span>
        <span className="font-medium">{temp(day.maxC, units)}</span>{' '}
        <span className="text-slate-500 dark:text-slate-400">{temp(day.minC, units)}</span>
      </span>
      <span className="flex h-4 items-center gap-0.5 text-[11px] font-medium text-sky-600 dark:text-sky-400">
        {rain >= 10 && (
          <>
            <Droplet className="size-2.5" aria-hidden />
            {Math.round(rain)}%
          </>
        )}
      </span>
    </div>
  )
}

function SpreadValue({ degreesC, units }: { degreesC: number; units: Units }) {
  const d = units === 'imperial' ? degreesC * 1.8 : degreesC
  const tone =
    degreesC <= 2 ? 'text-emerald-700 dark:text-emerald-400' : degreesC <= 4 ? 'text-amber-700 dark:text-amber-400' : 'text-rose-700 dark:text-rose-400'
  return <span className={`font-medium ${tone}`}>{Math.round(d)}°</span>
}

function Dash({ title }: { title?: string }) {
  return (
    <span className="block text-center text-slate-400" title={title}>
      –
    </span>
  )
}
