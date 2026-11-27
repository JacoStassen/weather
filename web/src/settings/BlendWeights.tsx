import { useEffect, useState } from 'react'
import { Scale } from 'lucide-react'
import { api } from '../api/client'
import type { BlendReport } from '../types'
import { useSelectedLocation, useSettings, type Units } from '../store/settings'
import { ago, placeLabel } from '../lib/format'
import { Card, ErrorNote, Spinner } from '../components/ui'

/**
 * How "Our blend" is made for the selected place: each source's share, and
 * how accurate it has been against the nearest weather station.
 */
export function BlendWeights() {
  const location = useSelectedLocation()
  const providers = useSettings((s) => s.serverProviders)
  const units = useSettings((s) => s.units)
  const [report, setReport] = useState<BlendReport | null>(null)
  const [error, setError] = useState<string>()

  const lat = location?.lat
  const lon = location?.lon
  const hasBlend = providers.some((p) => p.id === 'blend')
  useEffect(() => {
    if (lat == null || lon == null || !hasBlend) return
    const ctl = new AbortController()
    setReport(null)
    setError(undefined)
    api
      .blend({ lat, lon }, ctl.signal)
      .then(setReport)
      .catch((e: Error) => !ctl.signal.aborted && setError(e.message))
    return () => ctl.abort()
  }, [lat, lon, hasBlend])

  if (!location || !hasBlend) return null
  const name = (id: string) => providers.find((p) => p.id === id)?.name ?? id
  const rows = report ? [...report.sources].sort((a, b) => b.weight - a.weight) : []
  const learning = rows.some((r) => !r.measured)

  return (
    <Card title="How our blend is weighted" icon={Scale}>
      <p className="mb-3 text-sm text-slate-500 dark:text-slate-400">
        “Our blend” averages the sources below for {placeLabel(location)}. Every hour the server notes what each one predicts 6, 24
        and 48 hours ahead at the nearest airport weather station, then checks it against the station’s thermometer. The smaller a
        source’s typical error, the more it counts (a source half as far off counts about four times as much).
      </p>

      {error && <ErrorNote message={error} />}
      {!report && !error && <Spinner className="size-5" />}

      {report && (
        <>
          <p className="mb-3 text-sm">
            {report.station ? (
              <>
                Checked against <span className="font-medium">{report.station.name || report.station.id}</span> ({report.station.id},{' '}
                {Math.round(report.station.distanceKm)} km away)
                {report.lastReading && (
                  <span className="text-slate-500 dark:text-slate-400"> · last reading {ago(report.lastReading)}</span>
                )}
                .
              </>
            ) : (
              <span className="text-slate-500 dark:text-slate-400">
                No weather station matched yet. The server looks for one within 50 km within the hour; until then the blend uses
                starting estimates.
              </span>
            )}
          </p>

          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-slate-500 dark:text-slate-400">
                <th className="py-1.5 font-medium">Source</th>
                <th className="w-[38%] py-1.5 font-medium">Share</th>
                <th className="py-1.5 text-right font-medium">Typical error</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.id} className="border-t border-slate-100 dark:border-slate-800">
                  <td className="py-2 pr-2">
                    <span className="block font-medium">{name(r.id)}</span>
                    <span className="text-xs text-slate-500 dark:text-slate-400">
                      {r.measured
                        ? `${r.samples} readings${r.bias != null ? ` · ${biasLabel(r.bias, units)}` : ''}`
                        : r.samples > 0
                          ? `${r.samples} of 24 readings so far`
                          : 'no readings yet'}
                    </span>
                  </td>
                  <td className="py-2 pr-2">
                    <span className="flex items-center gap-2">
                      <span className="h-1.5 flex-1 rounded-full bg-slate-100 dark:bg-slate-800">
                        <span className="block h-full rounded-full bg-sky-500" style={{ width: `${r.weight * 100}%` }} />
                      </span>
                      <span className="w-9 text-right tabular-nums">{Math.round(r.weight * 100)}%</span>
                    </span>
                  </td>
                  <td className="py-2 text-right tabular-nums">
                    ±{degrees(r.mae, units)}
                    {!r.measured && <span className="block text-xs text-slate-500 dark:text-slate-400">estimate</span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          <p className="mt-3 text-xs text-slate-500 dark:text-slate-400">
            {learning
              ? 'A source uses its own record once it has 24 checked readings here, usually within a day; until then it starts from an estimate based on published global accuracy. '
              : ''}
            Only temperature is checked. The record leans on about the last week, so it follows the seasons.
          </p>
        </>
      )}
    </Card>
  )
}

/** A temperature difference in the chosen units. */
function degrees(c: number, units: Units): string {
  const v = units === 'imperial' ? c * 1.8 : c
  return `${v.toFixed(1)}°`
}

function biasLabel(bias: number, units: Units): string {
  if (Math.abs(bias) < 0.3) return 'no lean warm or cold'
  return `runs ${degrees(Math.abs(bias), units)} ${bias > 0 ? 'warm' : 'cold'}`
}
