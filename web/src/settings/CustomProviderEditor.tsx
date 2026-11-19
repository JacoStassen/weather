import { useMemo, useState } from 'react'
import { CircleCheck, FlaskConical, Plus, Trash2 } from 'lucide-react'
import { api } from '../api/client'
import { getApiKeySecret, type StoredCustomProvider } from '../db/repo'
import { useSelectedLocation, useSettings } from '../store/settings'
import type { Condition, CurrentMapping, CustomProviderTestResult, Transform } from '../types'
import { CONDITIONS, conditionLabel } from '../lib/conditions'
import { placeLabel } from '../lib/format'
import { ErrorNote, Spinner, buttonClass, inputClass } from '../components/ui'
import { EMPTY_MAPPING, PRESETS } from './presets'

type Field = keyof CurrentMapping
type Group = 'temp' | 'speed' | 'pressure' | 'length' | 'percent' | 'none'

const FIELDS: { key: Field; label: string; group: Group; hint?: string }[] = [
  { key: 'temperatureC', label: 'Temperature', group: 'temp' },
  { key: 'feelsLikeC', label: 'Feels like', group: 'temp' },
  { key: 'humidityPct', label: 'Humidity', group: 'percent' },
  { key: 'windSpeedMs', label: 'Wind speed', group: 'speed' },
  { key: 'windGustMs', label: 'Wind gusts', group: 'speed' },
  { key: 'windDirDeg', label: 'Wind direction (°)', group: 'none' },
  { key: 'pressureHpa', label: 'Pressure', group: 'pressure' },
  { key: 'precipMm', label: 'Precipitation', group: 'length' },
  { key: 'cloudCoverPct', label: 'Cloud cover', group: 'percent' },
  { key: 'uvIndex', label: 'UV index', group: 'none' },
  { key: 'condition', label: 'Condition', group: 'none', hint: 'text or code, mapped below' },
  { key: 'isDay', label: 'Is day', group: 'none', hint: 'true/false or 1/0' },
  { key: 'observedAt', label: 'Observed at', group: 'none', hint: 'unix seconds or ISO time' },
]

const UNIT_OPTIONS: Record<Group, { value: Transform | ''; label: string }[]> = {
  temp: [
    { value: '', label: '°C' },
    { value: 'fahrenheit_to_celsius', label: '°F' },
    { value: 'kelvin_to_celsius', label: 'K' },
  ],
  speed: [
    { value: '', label: 'm/s' },
    { value: 'kmh_to_ms', label: 'km/h' },
    { value: 'mph_to_ms', label: 'mph' },
    { value: 'knots_to_ms', label: 'knots' },
  ],
  pressure: [
    { value: '', label: 'hPa' },
    { value: 'inhg_to_hpa', label: 'inHg' },
  ],
  length: [
    { value: '', label: 'mm' },
    { value: 'inches_to_mm', label: 'in' },
  ],
  percent: [
    { value: '', label: '%' },
    { value: 'fraction_to_percent', label: '0–1' },
  ],
  none: [],
}

interface Draft {
  name: string
  slug: string
  slugTouched: boolean
  urlTemplate: string
  headers: [string, string][]
  apiKeyId: string
  mapping: CurrentMapping
  conditionMap: [string, Condition][]
}

const slugify = (s: string) =>
  s
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48)

function toDraft(p?: StoredCustomProvider): Draft {
  if (!p)
    return {
      name: '',
      slug: '',
      slugTouched: false,
      urlTemplate: '',
      headers: [],
      apiKeyId: '',
      mapping: EMPTY_MAPPING,
      conditionMap: [],
    }
  return {
    name: p.config.name,
    slug: p.config.slug,
    slugTouched: true,
    urlTemplate: p.config.urlTemplate,
    headers: Object.entries(p.config.headers ?? {}) as [string, string][],
    apiKeyId: p.apiKeyId ?? '',
    mapping: { ...EMPTY_MAPPING, ...p.config.mapping },
    conditionMap: Object.entries(p.config.conditionMap ?? {}) as [string, Condition][],
  }
}

function fromDraft(d: Draft): StoredCustomProvider {
  return {
    apiKeyId: d.apiKeyId || null,
    config: {
      slug: d.slug,
      name: d.name.trim(),
      urlTemplate: d.urlTemplate.trim(),
      headers: Object.fromEntries(d.headers.filter(([k]) => k.trim()).map(([k, v]) => [k.trim(), v])),
      mapping: Object.fromEntries(
        Object.entries(d.mapping).map(([k, v]) => [k, v?.path.trim() ? { ...v, path: v.path.trim() } : null]),
      ) as unknown as CurrentMapping,
      conditionMap: Object.fromEntries(d.conditionMap.filter(([k]) => k.trim()).map(([k, v]) => [k.trim(), v])),
    },
  }
}

/** Mirrors the server's checks so a bad config can't block syncing the others. */
function validate(d: Draft, takenSlugs: string[]): string | null {
  if (!d.name.trim()) return 'Give the source a name.'
  if (!/^[a-z0-9-]{1,48}$/.test(d.slug)) return 'The id may only use a–z, 0–9 and “-”.'
  if (takenSlugs.includes(d.slug)) return `Another custom source already uses the id “${d.slug}”.`
  const url = d.urlTemplate.trim()
  if (!/^https?:\/\//.test(url)) return 'The URL must start with http:// or https://.'
  if (!url.includes('{lat}') || !url.includes('{lon}')) return 'The URL must contain {lat} and {lon}.'
  if (!d.mapping.temperatureC?.path.trim()) return 'Map at least the temperature field.'
  return null
}

export function CustomProviderEditor({
  existing,
  onDone,
}: {
  existing?: StoredCustomProvider
  onDone: () => void
}) {
  const apiKeys = useSettings((s) => s.apiKeys)
  const customProviders = useSettings((s) => s.customProviders)
  const save = useSettings((s) => s.saveCustomProvider)
  const location = useSelectedLocation()

  const [d, setD] = useState<Draft>(() => toDraft(existing))
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [test, setTest] = useState<{ loading: boolean; result?: CustomProviderTestResult; error?: string }>({
    loading: false,
  })

  const takenSlugs = useMemo(
    () => customProviders.map((p) => p.config.slug).filter((s) => s !== existing?.config.slug),
    [customProviders, existing],
  )
  const update = (patch: Partial<Draft>) => setD((prev) => ({ ...prev, ...patch }))
  const setField = (key: Field, patch: { path?: string; transform?: Transform | null }) =>
    setD((prev) => {
      const cur = prev.mapping[key] ?? { path: '', transform: null }
      return { ...prev, mapping: { ...prev.mapping, [key]: { ...cur, ...patch } } }
    })

  const applyPreset = (label: string) => {
    const p = PRESETS.find((x) => x.label === label)
    if (!p) return
    setD((prev) => ({
      ...prev,
      name: p.name,
      slug: takenSlugs.includes(p.slug) ? `${p.slug}-2` : p.slug,
      slugTouched: true,
      urlTemplate: p.urlTemplate,
      headers: Object.entries(p.headers ?? {}),
      mapping: p.mapping,
      conditionMap: Object.entries(p.conditionMap),
    }))
  }

  const usesKey = d.urlTemplate.includes('{apiKey}') || d.headers.some(([, v]) => v.includes('{apiKey}'))
  const point = location ?? { lat: 52.37, lon: 4.9, name: 'Amsterdam', region: null, country: 'The Netherlands' }

  const runTest = async () => {
    const problem = validate(d, takenSlugs)
    if (problem) return setError(problem)
    setError(null)
    setTest({ loading: true })
    try {
      const { config, apiKeyId } = fromDraft(d)
      const apiKey = apiKeyId ? await getApiKeySecret(apiKeyId) : null
      const result = await api.testCustomProvider({ ...config, apiKey }, { lat: point.lat, lon: point.lon })
      setTest({ loading: false, result })
    } catch (err) {
      setTest({ loading: false, error: err instanceof Error ? err.message : String(err) })
    }
  }

  const onSave = async () => {
    const problem = validate(d, takenSlugs)
    if (problem) return setError(problem)
    if (usesKey && !d.apiKeyId) return setError('The URL or headers use {apiKey}: choose a key.')
    setSaving(true)
    try {
      await save(fromDraft(d), existing?.config.slug)
      onDone()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="space-y-6">
      {!existing && (
        <label className="block">
          <span className="text-sm font-medium">Start from</span>
          <select className={`${inputClass} mt-1`} defaultValue="" onChange={(e) => applyPreset(e.target.value)}>
            <option value="">Blank</option>
            {PRESETS.map((p) => (
              <option key={p.label}>{p.label}</option>
            ))}
          </select>
        </label>
      )}

      <div className="grid gap-4 sm:grid-cols-2">
        <label className="block">
          <span className="text-sm font-medium">Name</span>
          <input
            className={`${inputClass} mt-1`}
            value={d.name}
            onChange={(e) => update({ name: e.target.value, ...(d.slugTouched ? {} : { slug: slugify(e.target.value) }) })}
            placeholder="My weather API"
          />
        </label>
        <label className="block">
          <span className="text-sm font-medium">Id</span>
          <div className="mt-1 flex items-center rounded-lg border border-slate-300 bg-slate-50 text-sm focus-within:border-sky-500 focus-within:ring-2 focus-within:ring-sky-500/30 dark:border-slate-700 dark:bg-slate-900">
            <span className="pl-3 text-slate-500">custom:</span>
            <input
              className="w-full bg-transparent px-1 py-2 focus:outline-none"
              value={d.slug}
              onChange={(e) => update({ slug: e.target.value, slugTouched: true })}
              placeholder="my-weather-api"
            />
          </div>
        </label>
      </div>

      <label className="block">
        <span className="text-sm font-medium">Request URL</span>
        <input
          className={`${inputClass} mt-1 font-mono text-xs`}
          value={d.urlTemplate}
          onChange={(e) => update({ urlTemplate: e.target.value })}
          placeholder="https://api.example.com/current?lat={lat}&lon={lon}&key={apiKey}"
          spellCheck={false}
        />
        <span className="mt-1 block text-xs text-slate-500">
          Placeholders: <code>{'{lat}'}</code>, <code>{'{lon}'}</code>, <code>{'{apiKey}'}</code>. The response must be JSON.
        </span>
      </label>

      <div className="grid gap-4 sm:grid-cols-2">
        <label className="block">
          <span className="text-sm font-medium">API key</span>
          <select className={`${inputClass} mt-1`} value={d.apiKeyId} onChange={(e) => update({ apiKeyId: e.target.value })}>
            <option value="">None</option>
            {apiKeys.map((k) => (
              <option key={k.id} value={k.id}>
                {k.label} (…{k.hint})
              </option>
            ))}
          </select>
          {apiKeys.length === 0 && <span className="mt-1 block text-xs text-slate-500">Add keys in the API keys section.</span>}
        </label>
      </div>

      <fieldset>
        <legend className="text-sm font-medium">Headers</legend>
        <div className="mt-1 space-y-2">
          {d.headers.map(([k, v], i) => (
            <div key={i} className="flex gap-2">
              <input
                className={`${inputClass} font-mono text-xs`}
                value={k}
                placeholder="X-Api-Key"
                aria-label="Header name"
                onChange={(e) => update({ headers: d.headers.map((h, j) => (j === i ? [e.target.value, h[1]] : h)) })}
              />
              <input
                className={`${inputClass} font-mono text-xs`}
                value={v}
                placeholder="{apiKey}"
                aria-label="Header value"
                onChange={(e) => update({ headers: d.headers.map((h, j) => (j === i ? [h[0], e.target.value] : h)) })}
              />
              <button
                type="button"
                className={buttonClass.danger}
                aria-label="Remove header"
                onClick={() => update({ headers: d.headers.filter((_, j) => j !== i) })}
              >
                <Trash2 className="size-4" />
              </button>
            </div>
          ))}
          <button type="button" className={`${buttonClass.ghost} text-sm`} onClick={() => update({ headers: [...d.headers, ['', '']] })}>
            <Plus className="size-4" /> Add header
          </button>
        </div>
      </fieldset>

      <fieldset>
        <legend className="text-sm font-medium">Field mapping</legend>
        <p className="mt-0.5 text-xs text-slate-500">
          Paths into the JSON response, like <code>current.temp</code> or <code>data[0].wind.speed</code>. Leave a field empty if the
          API doesn’t have it.
        </p>
        <div className="mt-2 space-y-2">
          {FIELDS.map(({ key, label, group, hint }) => {
            const v = d.mapping[key]
            const units = UNIT_OPTIONS[group]
            return (
              <div key={key} className="grid grid-cols-[7.5rem_1fr_5.5rem] items-center gap-2 sm:grid-cols-[10rem_1fr_6.5rem]">
                <label htmlFor={`f-${key}`} className="text-sm">
                  {label}
                  {key === 'temperatureC' && <span className="text-rose-600"> *</span>}
                </label>
                <input
                  id={`f-${key}`}
                  className={`${inputClass} font-mono text-xs`}
                  value={v?.path ?? ''}
                  placeholder={hint ?? ''}
                  spellCheck={false}
                  onChange={(e) => setField(key, { path: e.target.value })}
                />
                {units.length > 0 ? (
                  <select
                    className={`${inputClass} px-2`}
                    aria-label={`${label} unit`}
                    value={v?.transform ?? ''}
                    onChange={(e) => setField(key, { transform: (e.target.value || null) as Transform | null })}
                  >
                    {units.map((u) => (
                      <option key={u.value} value={u.value}>
                        {u.label}
                      </option>
                    ))}
                  </select>
                ) : (
                  <span />
                )}
              </div>
            )
          })}
        </div>
      </fieldset>

      {d.mapping.condition?.path && (
        <fieldset>
          <legend className="text-sm font-medium">Condition values</legend>
          <p className="mt-0.5 text-xs text-slate-500">What the API’s condition values mean. Matching ignores case.</p>
          <div className="mt-2 space-y-2">
            {d.conditionMap.map(([k, v], i) => (
              <div key={i} className="flex gap-2">
                <input
                  className={`${inputClass} font-mono text-xs`}
                  value={k}
                  placeholder="Rain"
                  aria-label="Source value"
                  onChange={(e) => update({ conditionMap: d.conditionMap.map((c, j) => (j === i ? [e.target.value, c[1]] : c)) })}
                />
                <select
                  className={inputClass}
                  value={v}
                  aria-label="Condition"
                  onChange={(e) =>
                    update({ conditionMap: d.conditionMap.map((c, j) => (j === i ? [c[0], e.target.value as Condition] : c)) })
                  }
                >
                  {CONDITIONS.map((c) => (
                    <option key={c} value={c}>
                      {conditionLabel(c)}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className={buttonClass.danger}
                  aria-label="Remove value"
                  onClick={() => update({ conditionMap: d.conditionMap.filter((_, j) => j !== i) })}
                >
                  <Trash2 className="size-4" />
                </button>
              </div>
            ))}
            <button
              type="button"
              className={`${buttonClass.ghost} text-sm`}
              onClick={() => update({ conditionMap: [...d.conditionMap, ['', 'clear']] })}
            >
              <Plus className="size-4" /> Add value
            </button>
          </div>
        </fieldset>
      )}

      <TestResult test={test} placeName={placeLabel(point)} />

      {error && <ErrorNote message={error} />}
      <div className="flex flex-wrap items-center gap-2">
        <button type="button" className={buttonClass.primary} onClick={onSave} disabled={saving}>
          {saving && <Spinner className="size-4 text-white" />}
          Save
        </button>
        <button type="button" className={buttonClass.secondary} onClick={runTest} disabled={test.loading}>
          {test.loading ? <Spinner className="size-4" /> : <FlaskConical className="size-4" />}
          Test
        </button>
        <button type="button" className={buttonClass.secondary} onClick={onDone}>
          Cancel
        </button>
      </div>
    </div>
  )
}

function TestResult({
  test,
  placeName,
}: {
  test: { loading: boolean; result?: CustomProviderTestResult; error?: string }
  placeName: string
}) {
  if (test.error) return <ErrorNote message={test.error} />
  const r = test.result
  if (!r) return null
  const raw = r.raw != null ? JSON.stringify(r.raw, null, 2) : null
  return (
    <div className="space-y-2 rounded-xl border border-slate-200 p-3 text-sm dark:border-slate-700">
      <p className="text-xs text-slate-500">Test request for {placeName}</p>
      {r.error ? (
        <ErrorNote message={r.error} />
      ) : (
        r.current && (
          <div className="flex items-start gap-2 text-emerald-700 dark:text-emerald-400">
            <CircleCheck className="mt-0.5 size-4 shrink-0" aria-hidden />
            <div>
              <p className="font-medium">
                {r.current.temperatureC.toFixed(1)} °C, {conditionLabel(r.current.condition)}
              </p>
              <p className="text-xs text-slate-600 dark:text-slate-400">
                {Object.entries(r.current)
                  .filter(([k, v]) => v != null && !['source', 'temperatureC', 'condition'].includes(k))
                  .map(([k, v]) => `${k}: ${typeof v === 'number' ? Math.round(v * 10) / 10 : String(v)}`)
                  .join(' · ')}
              </p>
            </div>
          </div>
        )
      )}
      {raw && (
        <details>
          <summary className="cursor-pointer text-xs font-medium text-slate-600 dark:text-slate-400">Raw response</summary>
          <pre className="mt-2 max-h-80 overflow-auto rounded-lg bg-slate-50 p-3 font-mono text-[11px] leading-relaxed dark:bg-slate-950">
            {raw.length > 20000 ? `${raw.slice(0, 20000)}\n…` : raw}
          </pre>
        </details>
      )}
    </div>
  )
}
