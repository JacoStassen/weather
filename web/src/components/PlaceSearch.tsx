import { useEffect, useRef, useState } from 'react'
import { MapPin, Search } from 'lucide-react'
import { api } from '../api/client'
import type { Place } from '../types'
import { placeLabel } from '../lib/format'
import { ErrorNote, Spinner, inputClass } from './ui'

/** Search box with live results from the geocoding endpoint. */
export function PlaceSearch({ onPick, autoFocus }: { onPick: (p: Place) => void; autoFocus?: boolean }) {
  const [q, setQ] = useState('')
  const [results, setResults] = useState<Place[]>([])
  const [state, setState] = useState<'idle' | 'loading' | 'done' | 'error'>('idle')
  const [active, setActive] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (autoFocus) inputRef.current?.focus()
  }, [autoFocus])

  useEffect(() => {
    if (q.trim().length < 2) {
      setResults([])
      setState('idle')
      return
    }
    const ctrl = new AbortController()
    const t = setTimeout(async () => {
      setState('loading')
      try {
        const r = await api.geocode(q, ctrl.signal)
        setResults(r)
        setActive(0)
        setState('done')
      } catch {
        if (!ctrl.signal.aborted) setState('error')
      }
    }, 250)
    return () => {
      clearTimeout(t)
      ctrl.abort()
    }
  }, [q])

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setActive((a) => Math.min(a + 1, results.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActive((a) => Math.max(a - 1, 0))
    } else if (e.key === 'Enter' && results[active]) {
      e.preventDefault()
      onPick(results[active])
    }
  }

  return (
    <div>
      <div className="relative">
        <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-slate-400" aria-hidden />
        <input
          ref={inputRef}
          type="search"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Search for a city or town"
          className={`${inputClass} pl-9`}
          aria-label="Search places"
          role="combobox"
          aria-expanded={results.length > 0}
          aria-controls="place-results"
        />
        {state === 'loading' && <Spinner className="absolute top-1/2 right-3 size-4 -translate-y-1/2" />}
      </div>

      {state === 'error' && <ErrorNote className="mt-3" message="Search failed. Is the weather server running?" />}
      {state === 'done' && results.length === 0 && (
        <p className="mt-3 text-sm text-slate-500">No places found for “{q.trim()}”.</p>
      )}
      {results.length > 0 && (
        <ul id="place-results" role="listbox" className="mt-2 max-h-72 overflow-y-auto">
          {results.map((p, i) => (
            <li key={`${p.lat},${p.lon},${i}`} role="option" aria-selected={i === active}>
              <button
                type="button"
                onClick={() => onPick(p)}
                onMouseEnter={() => setActive(i)}
                className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-sm ${
                  i === active ? 'bg-sky-50 dark:bg-sky-950' : ''
                }`}
              >
                <MapPin className="size-4 shrink-0 text-slate-400" aria-hidden />
                <span className="min-w-0">
                  <span className="block truncate font-medium">{p.name}</span>
                  <span className="block truncate text-xs text-slate-500">
                    {placeLabel({ name: '', region: p.region, country: p.country }).replace(/^, /, '')}
                  </span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
