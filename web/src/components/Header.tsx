import { useEffect, useRef, useState } from 'react'
import { ArrowLeft, Check, ChevronDown, CloudSun, LocateFixed, MapPin, Navigation, Plus, RefreshCw, Settings, X } from 'lucide-react'
import { useSelectedLocation, useSettings } from '../store/settings'
import type { Place } from '../types'
import { PlaceSearch } from './PlaceSearch'
import { LocateButton, OrDivider } from './LocateButton'
import { buttonClass } from './ui'

export function Header({
  view,
  onRefresh,
  refreshing,
}: {
  view: 'dashboard' | 'settings'
  onRefresh?: () => void
  refreshing?: boolean
}) {
  return (
    <header className="flex items-center justify-between gap-2 py-3">
      {view === 'settings' ? (
        <a href="#/" className={`${buttonClass.ghost} -ml-2 gap-1.5 pr-3 text-sm font-medium`}>
          <ArrowLeft className="size-4" aria-hidden />
          Back
        </a>
      ) : (
        <div className="flex min-w-0 items-center gap-2">
          <CloudSun className="size-6 shrink-0 text-sky-500" aria-hidden />
          <LocationMenu />
        </div>
      )}
      <div className="flex items-center gap-1">
        {view === 'dashboard' && onRefresh && (
          <button type="button" onClick={onRefresh} className={buttonClass.ghost} aria-label="Refresh" disabled={refreshing}>
            <RefreshCw className={`size-5 ${refreshing ? 'animate-spin' : ''}`} />
          </button>
        )}
        {view === 'dashboard' && (
          <a href="#/settings" className={buttonClass.ghost} aria-label="Settings">
            <Settings className="size-5" />
          </a>
        )}
      </div>
    </header>
  )
}

function LocationMenu() {
  const location = useSelectedLocation()
  const locations = useSettings((s) => s.locations)
  const selectLocation = useSettings((s) => s.selectLocation)
  const [open, setOpen] = useState(false)
  const [searching, setSearching] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const onDown = (e: PointerEvent) => !ref.current?.contains(e.target as Node) && setOpen(false)
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false)
    document.addEventListener('pointerdown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('pointerdown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  if (!location) return <span className="text-lg font-semibold">Weather</span>

  return (
    <div ref={ref} className="relative min-w-0">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex max-w-full items-center gap-1 rounded-lg px-1.5 py-1 text-lg font-semibold hover:bg-slate-200/60 dark:hover:bg-slate-800"
        aria-haspopup="menu"
        aria-expanded={open}
      >
        {location.kind === 'device' && (
          <Navigation className="size-4 shrink-0 fill-current text-sky-500" aria-label="Your location" />
        )}
        <span className="truncate">{location.name}</span>
        <ChevronDown className="size-4 shrink-0 text-slate-500" aria-hidden />
      </button>
      {open && (
        <div
          role="menu"
          className="absolute top-full left-0 z-20 mt-1 w-72 max-w-[calc(100vw-2rem)] rounded-xl border border-slate-200 bg-white p-1.5 shadow-lg dark:border-slate-700 dark:bg-slate-900"
        >
          {locations.map((l) => (
            <button
              key={l.id}
              type="button"
              role="menuitemradio"
              aria-checked={l.id === location.id}
              onClick={() => {
                void selectLocation(l.id)
                setOpen(false)
              }}
              className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-sm hover:bg-slate-100 dark:hover:bg-slate-800"
            >
              {l.kind === 'device' ? (
                <LocateFixed className="size-4 shrink-0 text-sky-500" aria-label="Your location" />
              ) : (
                <MapPin className="size-4 shrink-0 text-slate-400" aria-hidden />
              )}
              <span className="min-w-0 flex-1 truncate">
                {l.name}
                {l.country && <span className="text-slate-500">, {l.country}</span>}
              </span>
              {l.id === location.id && <Check className="size-4 text-sky-600" aria-hidden />}
            </button>
          ))}
          <div className="my-1 border-t border-slate-200 dark:border-slate-700" />
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setOpen(false)
              setSearching(true)
            }}
            className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-sm font-medium text-sky-700 hover:bg-slate-100 dark:text-sky-400 dark:hover:bg-slate-800"
          >
            <Plus className="size-4" aria-hidden />
            Add location
          </button>
        </div>
      )}
      {searching && <AddLocationDialog onClose={() => setSearching(false)} />}
    </div>
  )
}

export function AddLocationDialog({ onClose }: { onClose: () => void }) {
  const addLocation = useSettings((s) => s.addLocation)

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [onClose])

  const pick = async (p: Place) => {
    await addLocation(p)
    onClose()
  }

  return (
    <div
      className="fixed inset-0 z-30 flex items-start justify-center bg-slate-950/40 p-4 pt-[10vh] backdrop-blur-sm"
      onPointerDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Add location"
        className="w-full max-w-md rounded-2xl border border-slate-200 bg-white p-4 shadow-xl dark:border-slate-700 dark:bg-slate-900"
      >
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-base font-semibold">Add location</h2>
          <button type="button" onClick={onClose} className={buttonClass.ghost} aria-label="Close">
            <X className="size-4" />
          </button>
        </div>
        <LocateButton onDone={onClose} />
        <OrDivider />
        <PlaceSearch onPick={pick} autoFocus />
      </div>
    </div>
  )
}
