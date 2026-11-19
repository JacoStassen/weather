import { useEffect } from 'react'
import { LocateFixed } from 'lucide-react'
import { useSettings } from '../store/settings'
import { ErrorNote, Spinner, buttonClass } from './ui'

/** Asks for the device position and selects it. */
export function LocateButton({ onDone, className = '' }: { onDone?: () => void; className?: string }) {
  const locating = useSettings((s) => s.locating)
  const error = useSettings((s) => s.locateError)
  const locateDevice = useSettings((s) => s.locateDevice)

  // Don't greet the next visit with an old error.
  useEffect(() => () => useSettings.setState({ locateError: undefined }), [])

  if (!('geolocation' in navigator)) return null

  const onClick = async () => {
    await locateDevice()
    if (!useSettings.getState().locateError) onDone?.()
  }

  return (
    <div className={className}>
      <button type="button" className={`${buttonClass.secondary} w-full`} onClick={onClick} disabled={locating}>
        {locating ? <Spinner className="size-4" /> : <LocateFixed className="size-4 text-sky-600" aria-hidden />}
        {locating ? 'Finding you…' : 'Use my current location'}
      </button>
      {error && <ErrorNote message={error} className="mt-2" />}
    </div>
  )
}

/** Divider between "use my location" and search. */
export function OrDivider() {
  return (
    <div className="my-4 flex items-center gap-3 text-xs text-slate-400" aria-hidden>
      <span className="h-px flex-1 bg-slate-200 dark:bg-slate-800" />
      or search
      <span className="h-px flex-1 bg-slate-200 dark:bg-slate-800" />
    </div>
  )
}
