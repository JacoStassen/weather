import { Droplets, Gauge, Navigation, Sun, Umbrella, Wind } from 'lucide-react'
import type { ReactNode } from 'react'
import type { CurrentConditions, DailyPoint } from '../types'
import type { Loadable } from '../store/weather'
import type { Units } from '../store/settings'
import { conditionIcon, conditionLabel } from '../lib/conditions'
import { ago, compass, pct, precip, pressure, temp, wind } from '../lib/format'
import { ErrorNote, Skeleton } from '../components/ui'

export function CurrentCard({
  current,
  today,
  units,
  providerName,
}: {
  current: Loadable<CurrentConditions>
  today?: DailyPoint
  units: Units
  providerName: (id: string) => string
}) {
  const c = current.data
  const night = c ? !c.isDay : false

  return (
    <section
      className={`relative overflow-hidden rounded-2xl p-5 text-white shadow-sm sm:p-6 ${
        night ? 'bg-gradient-to-br from-indigo-950 via-slate-900 to-slate-800' : 'bg-gradient-to-br from-sky-500 to-blue-700'
      }`}
      aria-busy={current.loading}
    >
      {!c && current.loading && <CurrentSkeleton />}
      {!c && current.error && <ErrorNote message={current.error.message} className="!text-white" />}
      {c && (
        <>
          <div className="flex items-start justify-between gap-4">
            <div>
              <p className="text-6xl font-light tracking-tight sm:text-7xl">{temp(c.temperatureC, units)}</p>
              <p className="mt-1 text-lg font-medium">{conditionLabel(c.condition)}</p>
              <p className="mt-0.5 text-sm text-white/80">
                {c.feelsLikeC != null && <>Feels like {temp(c.feelsLikeC, units)}</>}
                {today && (
                  <>
                    {c.feelsLikeC != null && ' · '}H {temp(today.maxC, units)} L {temp(today.minC, units)}
                  </>
                )}
              </p>
            </div>
            <ConditionGlyph condition={c.condition} isDay={c.isDay} />
          </div>

          <dl className="mt-6 grid grid-cols-2 gap-x-4 gap-y-3 text-sm sm:grid-cols-3">
            <Stat icon={<WindIcon deg={c.windDirDeg} />} label="Wind">
              {wind(c.windSpeedMs, units)} {compass(c.windDirDeg)}
              {c.windGustMs != null && <span className="text-white/70"> · gusts {wind(c.windGustMs, units)}</span>}
            </Stat>
            <Stat icon={<Droplets className="size-4" />} label="Humidity">
              {pct(c.humidityPct)}
            </Stat>
            <Stat icon={<Gauge className="size-4" />} label="Pressure">
              {pressure(c.pressureHpa, units)}
            </Stat>
            <Stat icon={<Umbrella className="size-4" />} label="Precipitation">
              {precip(c.precipMm, units)}
            </Stat>
            <Stat icon={<Sun className="size-4" />} label="UV index">
              {c.uvIndex != null ? Math.round(c.uvIndex) : '–'}
            </Stat>
            <Stat icon={<Wind className="size-4" />} label="Cloud cover">
              {pct(c.cloudCoverPct)}
            </Stat>
          </dl>

          <p className="mt-5 text-xs text-white/70">
            {providerName(c.source.providerId)} · updated {ago(c.source.fetchedAt)}
            {current.error && <span className="ml-2 text-amber-200">Refresh failed: {current.error.message}</span>}
          </p>
        </>
      )}
    </section>
  )
}

function ConditionGlyph({ condition, isDay }: { condition: CurrentConditions['condition']; isDay: boolean }) {
  const Icon = conditionIcon(condition, isDay)
  return <Icon className="size-20 shrink-0 text-white/90 drop-shadow sm:size-24" strokeWidth={1.25} aria-hidden />
}

/** Arrow pointing where the wind blows to (meteorological degrees say where it comes from). */
function WindIcon({ deg }: { deg: number | null }) {
  if (deg == null) return <Wind className="size-4" />
  return <Navigation className="size-4" style={{ transform: `rotate(${deg + 180 - 45}deg)` }} />
}

function Stat({ icon, label, children }: { icon: ReactNode; label: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="flex items-center gap-1.5 text-xs text-white/70">
        <span aria-hidden>{icon}</span>
        {label}
      </dt>
      <dd className="mt-0.5 truncate font-medium">{children}</dd>
    </div>
  )
}

function CurrentSkeleton() {
  return (
    <div className="space-y-3 opacity-60">
      <Skeleton className="h-16 w-32 !bg-white/30" />
      <Skeleton className="h-5 w-40 !bg-white/30" />
      <Skeleton className="mt-6 h-24 w-full !bg-white/20" />
    </div>
  )
}
