import { useCallback, useEffect } from 'react'
import { MapPin } from 'lucide-react'
import { useSelectedLocation, useSettings } from '../store/settings'
import { useWeather } from '../store/weather'
import { Header } from '../components/Header'
import { PlaceSearch } from '../components/PlaceSearch'
import { LocateButton, OrDivider } from '../components/LocateButton'
import { Card } from '../components/ui'
import { CurrentCard } from './CurrentCard'
import { HourlyStrip } from './HourlyStrip'
import { DailyList } from './DailyList'
import { AlertsPanel } from './AlertsPanel'
import { TodayCard } from './TodayCard'
import { MarineSection } from './MarineSection'

const REFRESH_MS = 10 * 60 * 1000

export function Dashboard() {
  const location = useSelectedLocation()
  const providerId = useSettings((s) => s.activeProviderId)
  const units = useSettings((s) => s.units)
  const serverProviders = useSettings((s) => s.serverProviders)
  const { current, forecast, alerts, marine, loadedAt, load, clear } = useWeather()

  const refreshDeviceLocation = useSettings((s) => s.refreshDeviceLocation)

  // "My location" follows the device: re-check on open and when the app returns
  // to the foreground (only if permission was already granted; never prompts).
  useEffect(() => {
    if (location?.kind !== 'device') return
    void refreshDeviceLocation()
    const onVisible = () => document.visibilityState === 'visible' && void refreshDeviceLocation()
    document.addEventListener('visibilitychange', onVisible)
    return () => document.removeEventListener('visibilitychange', onVisible)
  }, [location?.kind, refreshDeviceLocation])

  const refresh = useCallback(() => {
    if (location) void load({ lat: location.lat, lon: location.lon }, providerId)
  }, [location, providerId, load])

  useEffect(() => {
    if (location) refresh()
    else clear()
  }, [location, refresh, clear])

  // Refresh periodically, and when the app comes back to the foreground with stale data.
  useEffect(() => {
    const id = setInterval(refresh, REFRESH_MS)
    const onVisible = () => {
      if (document.visibilityState === 'visible' && loadedAt && Date.now() - loadedAt > REFRESH_MS) refresh()
    }
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      clearInterval(id)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [refresh, loadedAt])

  const providerName = (id: string) => serverProviders.find((p) => p.id === id)?.name ?? id
  const loading = current.loading || forecast.loading || alerts.loading
  const hasAlerts = (alerts.data?.alerts.length ?? 0) > 0

  return (
    <>
      <Header view="dashboard" onRefresh={location ? refresh : undefined} refreshing={loading} />
      {!location ? (
        <FirstLocation />
      ) : (
        <main className="wx-rise mt-2 grid gap-4 lg:grid-cols-3">
          {hasAlerts && (
            <div className="lg:col-span-3">
              <AlertsPanel alerts={alerts} timeZone={forecast.data?.timezone} />
            </div>
          )}
          {/* Cards stretch to the tallest one in their row, so the edges line up. */}
          <div className="lg:col-span-2 *:h-full">
            <CurrentCard
              current={current}
              today={forecast.data?.daily[0]}
              units={units}
              providerName={providerName}
            />
          </div>
          <div className="flex flex-col gap-4 *:last:grow">
            {!hasAlerts && <AlertsPanel alerts={alerts} timeZone={forecast.data?.timezone} />}
            <TodayCard forecast={forecast} units={units} />
          </div>
          <div className="min-w-0 lg:col-span-3">
            <HourlyStrip forecast={forecast} units={units} />
          </div>
          <MarineSection marine={marine} units={units} timeZone={forecast.data?.timezone} />
          <div className="lg:col-span-3">
            <DailyList forecast={forecast} units={units} />
          </div>
        </main>
      )}
    </>
  )
}

function FirstLocation() {
  const addLocation = useSettings((s) => s.addLocation)
  return (
    <main className="mx-auto mt-10 max-w-md">
      <Card>
        <div className="mb-4 flex flex-col items-center text-center">
          <span className="mb-3 rounded-full bg-sky-100 p-3 text-sky-600 dark:bg-sky-950 dark:text-sky-400">
            <MapPin className="size-6" aria-hidden />
          </span>
          <h1 className="text-lg font-semibold">Where are you?</h1>
          <p className="mt-1 text-sm text-slate-500 dark:text-slate-400">
            Add a location to see its forecast. You can save several and switch between them.
          </p>
        </div>
        <LocateButton />
        <OrDivider />
        <PlaceSearch onPick={(p) => void addLocation(p)} />
      </Card>
    </main>
  )
}
