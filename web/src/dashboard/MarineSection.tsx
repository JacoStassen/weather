import { ArrowDown, ArrowDownToLine, ArrowUp, ArrowUpToLine, Info, Navigation, Waves } from 'lucide-react'
import type { MarineReport, MarineResponse, TideData, TideEvent } from '../types'
import type { Loadable } from '../store/weather'
import type { Units } from '../store/settings'
import { clock, compass, length, temp } from '../lib/format'
import { Card } from '../components/ui'
import { TimeSeriesChart, type SeriesPoint } from '../components/TimeSeriesChart'

const HOUR = 3600_000

/**
 * Tides and waves. Renders nothing unless the location is coastal: the marine
 * API has no data inland. Large lakes get waves but no tides.
 */
export function MarineSection({
  marine,
  units,
  timeZone,
}: {
  marine: Loadable<MarineResponse>
  units: Units
  timeZone?: string
}) {
  const report = marine.data?.coastal ? marine.data.report : null
  if (!report) return null
  const now = Date.now()

  if (!report.tides) {
    return (
      <div className="min-w-0 lg:col-span-3">
        <WavesCard report={report} units={units} timeZone={timeZone} now={now} wide />
      </div>
    )
  }
  return (
    <>
      <div className="min-w-0 lg:col-span-2 *:h-full">
        <TidesCard tides={report.tides} units={units} timeZone={timeZone} now={now} />
      </div>
      <div className="min-w-0 *:h-full">
        <WavesCard report={report} units={units} timeZone={timeZone} now={now} />
      </div>
    </>
  )
}

// ---- Tides -----------------------------------------------------------------

function TidesCard({ tides, units, timeZone, now }: { tides: TideData; units: Units; timeZone?: string; now: number }) {
  const from = now - 6 * HOUR
  const to = now + 30 * HOUR
  const series: SeriesPoint[] = tides.series
    .map((s) => ({ t: Date.parse(s.time), v: s.heightM }))
    .filter((p) => p.t >= from - HOUR && p.t <= to + HOUR)
  const events = tides.events.map((e) => ({ ...e, t: Date.parse(e.time) }))
  const upcoming = events.filter((e) => e.t > now).slice(0, 4)
  const level = heightAt(series, now)
  const rising = upcoming[0]?.kind === 'high'
  const fmtH = (v: number) => length(v, units, 2)
  const fmtT = (t: number) => clock(new Date(t).toISOString(), timeZone)

  return (
    <Card
      title="Tides"
      icon={Waves}
      action={
        <span className="flex items-center gap-1 text-[11px] text-slate-500 dark:text-slate-400" title="Modeled from Open-Meteo sea-level data, not tide-station predictions.">
          <Info className="size-3.5" aria-hidden />
          {tides.modeled ? 'Modeled · not for navigation' : 'Predicted'}
        </span>
      }
    >
      {level != null && upcoming[0] && (
        <p className="mb-3 flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <span className="text-2xl font-semibold">{fmtH(level)}</span>
          <span className="flex items-center gap-1 text-sm text-slate-600 dark:text-slate-300">
            {rising ? <ArrowUp className="size-4" aria-hidden /> : <ArrowDown className="size-4" aria-hidden />}
            {rising ? 'Rising' : 'Falling'}
          </span>
          <span className="text-sm text-slate-500 dark:text-slate-400">
            {upcoming[0].kind === 'high' ? 'High' : 'Low'} tide at {fmtT(upcoming[0].t)} ({until(upcoming[0].t - now)})
          </span>
        </p>
      )}

      <TimeSeriesChart
        points={series.filter((p) => p.t >= from && p.t <= to)}
        height={170}
        area
        now={now}
        ariaLabel={`Tide height for the next 30 hours. ${upcoming
          .map((e) => `${e.kind} tide ${fmtT(e.t)}, ${fmtH(e.heightM)}`)
          .join('; ')}.`}
        formatValue={fmtH}
        formatTick={(v) => length(v, units, 1)}
        formatTime={(t) => weekdayClock(t, timeZone)}
        xTicks={hourTicks(from, to, 6, timeZone)}
        markers={events
          .filter((e) => e.t >= from && e.t <= to)
          .map((e) => ({ t: e.t, v: e.heightM, label: fmtT(e.t), placement: e.kind === 'high' ? 'above' : 'below' }))}
      />

      {/* The table view: every value in the chart, readable without hovering. */}
      <ol className="mt-3 grid grid-cols-2 gap-2 sm:grid-cols-4" aria-label="Upcoming tides">
        {upcoming.map((e) => (
          <TideItem key={e.time} event={e} units={units} timeZone={timeZone} />
        ))}
      </ol>
    </Card>
  )
}

function TideItem({ event, units, timeZone }: { event: TideEvent; units: Units; timeZone?: string }) {
  const high = event.kind === 'high'
  const Icon = high ? ArrowUpToLine : ArrowDownToLine
  return (
    <li className="rounded-xl bg-slate-50 px-3 py-2 dark:bg-slate-800/60">
      <span className="flex items-center gap-1 text-xs text-slate-500 dark:text-slate-400">
        <Icon className="size-3.5" aria-hidden />
        {high ? 'High' : 'Low'} · {weekdayShort(event.time, timeZone)}
      </span>
      <span className="block text-sm font-semibold">{clock(event.time, timeZone)}</span>
      <span className="block text-xs text-slate-600 dark:text-slate-300">{length(event.heightM, units, 2)}</span>
    </li>
  )
}

// ---- Waves -----------------------------------------------------------------

function WavesCard({
  report,
  units,
  timeZone,
  now,
  wide = false,
}: {
  report: MarineReport
  units: Units
  timeZone?: string
  now: number
  wide?: boolean
}) {
  const waves = report.waves.map((w) => ({ ...w, t: Date.parse(w.time) }))
  const current = waves.reduce<(typeof waves)[number] | undefined>(
    (best, w) => (!best || Math.abs(w.t - now) < Math.abs(best.t - now) ? w : best),
    undefined,
  )
  const series = waves
    .filter((w) => w.waveHeightM != null && w.t >= now - HOUR && w.t <= now + 24 * HOUR)
    .map((w) => ({ t: w.t, v: w.waveHeightM! }))
  if (!current) return null

  return (
    <Card title="Waves" icon={Waves} className="h-full">
      <div className={wide ? 'grid gap-4 sm:grid-cols-[minmax(0,16rem)_1fr]' : ''}>
        <dl className="grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
          <div className="col-span-2">
            <dt className="text-xs text-slate-500 dark:text-slate-400">Wave height now</dt>
            <dd className="text-2xl font-semibold">{length(current.waveHeightM, units)}</dd>
          </div>
          <Stat label="Period">{current.wavePeriodS != null ? `${Math.round(current.wavePeriodS)} s` : '–'}</Stat>
          <Stat label="From">
            {current.waveDirDeg != null ? (
              <span className="flex items-center gap-1">
                <Navigation
                  className="size-3.5"
                  style={{ transform: `rotate(${current.waveDirDeg + 180 - 45}deg)` }}
                  aria-hidden
                />
                {compass(current.waveDirDeg)}
              </span>
            ) : (
              '–'
            )}
          </Stat>
          <Stat label="Swell">{length(current.swellHeightM, units)}</Stat>
          <Stat label="Water">{temp(current.seaTempC, units)}</Stat>
        </dl>
        {series.length > 2 && (
          <div className={wide ? '' : 'mt-4'}>
            <p className="mb-1 text-xs text-slate-500 dark:text-slate-400">Wave height, next 24 hours</p>
            <TimeSeriesChart
              points={series}
              height={110}
              now={now}
              ariaLabel={`Wave height over the next 24 hours, from ${length(Math.min(...series.map((p) => p.v)), units)} to ${length(
                Math.max(...series.map((p) => p.v)),
                units,
              )}.`}
              formatValue={(v) => length(v, units)}
              formatTime={(t) => weekdayClock(t, timeZone)}
              xTicks={hourTicks(now - HOUR, now + 24 * HOUR, 6, timeZone)}
            />
          </div>
        )}
      </div>
    </Card>
  )
}

function Stat({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt className="text-xs text-slate-500 dark:text-slate-400">{label}</dt>
      <dd className="font-medium">{children}</dd>
    </div>
  )
}

// ---- helpers ---------------------------------------------------------------

/** Linear interpolation of the series at `t`. */
function heightAt(series: SeriesPoint[], t: number): number | null {
  for (let i = 1; i < series.length; i++) {
    const a = series[i - 1]
    const b = series[i]
    if (a.t <= t && t <= b.t) return a.v + ((b.v - a.v) * (t - a.t)) / (b.t - a.t)
  }
  return null
}

function until(ms: number): string {
  const mins = Math.max(0, Math.round(ms / 60000))
  const h = Math.floor(mins / 60)
  return h ? `in ${h} h ${mins % 60} min` : `in ${mins} min`
}

/** Tick every `every` hours on the location's clock; midnight shows the weekday. */
function hourTicks(from: number, to: number, every: number, timeZone?: string): { t: number; label: string }[] {
  const hourOf = new Intl.DateTimeFormat('en-GB', { hour: 'numeric', hourCycle: 'h23', timeZone })
  const ticks: { t: number; label: string }[] = []
  for (let t = Math.ceil(from / HOUR) * HOUR; t <= to; t += HOUR) {
    const h = Number(hourOf.format(t))
    if (h % every !== 0) continue
    ticks.push({ t, label: h === 0 ? weekdayShort(new Date(t).toISOString(), timeZone) : clock(new Date(t).toISOString(), timeZone) })
  }
  return ticks
}

function weekdayShort(iso: string, timeZone?: string): string {
  return new Intl.DateTimeFormat(undefined, { weekday: 'short', timeZone }).format(new Date(iso))
}

function weekdayClock(t: number, timeZone?: string): string {
  return new Intl.DateTimeFormat(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit', timeZone }).format(t)
}
