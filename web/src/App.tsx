import { useEffect, useSyncExternalStore } from 'react'
import { useSettings } from './store/settings'
import { Dashboard } from './dashboard/Dashboard'
import { SettingsView } from './settings/SettingsView'
import { ErrorNote, Spinner } from './components/ui'
import { Backdrop } from './components/Ambience'

function useHashRoute(): 'dashboard' | 'settings' {
  const hash = useSyncExternalStore(
    (cb) => {
      window.addEventListener('hashchange', cb)
      return () => window.removeEventListener('hashchange', cb)
    },
    () => window.location.hash,
  )
  return hash.startsWith('#/settings') ? 'settings' : 'dashboard'
}

export default function App() {
  const status = useSettings((s) => s.status)
  const error = useSettings((s) => s.error)
  const init = useSettings((s) => s.init)
  const route = useHashRoute()

  useEffect(() => {
    void init()
  }, [init])

  useEffect(() => {
    // Block body: scrollTo returns a Promise in newer browsers, which React
    // would treat as an (invalid) cleanup function.
    window.scrollTo(0, 0)
  }, [route])

  return (
    <div className="mx-auto max-w-5xl px-4 pt-[max(0.5rem,env(safe-area-inset-top))] pb-10">
      <Backdrop />
      {status === 'loading' && (
        <div className="flex h-[60vh] items-center justify-center">
          <Spinner className="size-8" />
        </div>
      )}
      {status === 'error' && (
        <div className="mt-20">
          <ErrorNote message={`Couldn’t open the local database: ${error}`} />
        </div>
      )}
      {status === 'ready' && (route === 'settings' ? <SettingsView /> : <Dashboard />)}
    </div>
  )
}
