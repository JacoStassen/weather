import { useState, type FormEvent } from 'react'
import { Database, Eye, Palette, EyeOff, KeyRound, LockKeyhole, MapPin, Pencil, Plus, Ruler, Server, Trash2, Waypoints } from 'lucide-react'
import { DEFAULT_PROVIDER, useSettings, type Units } from '../store/settings'
import type { StoredCustomProvider } from '../db/repo'
import type { Capability } from '../types'
import { placeLabel } from '../lib/format'
import { Header, AddLocationDialog } from '../components/Header'
import { Card, ErrorNote, buttonClass, inputClass } from '../components/ui'
import { CustomProviderEditor } from './CustomProviderEditor'
import { CompareSources } from './CompareSources'
import { BlendWeights } from './BlendWeights'

export function SettingsView() {
  return (
    <>
      <Header view="settings" />
      <main className="mx-auto mt-2 max-w-2xl space-y-4">
        <h1 className="text-2xl font-semibold">Settings</h1>
        <StorageNotice />
        <ServerAccess />
        <DataSource />
        <CompareSources />
        <BlendWeights />
        <UnitsSection />
        <Appearance />
        <Locations />
        <CustomSources />
        <ApiKeys />
      </main>
    </>
  )
}

function StorageNotice() {
  const persistent = useSettings((s) => s.persistent)
  const syncError = useSettings((s) => s.syncError)
  return (
    <>
      {!persistent && (
        <ErrorNote message="Settings can’t be saved in this tab (private window, or the app is open in another tab). Changes last until you close it." />
      )}
      {syncError && <ErrorNote message={`Couldn’t reach the weather server: ${syncError}`} />}
    </>
  )
}

/**
 * Only on a server reachable from the internet. The token unlocks custom
 * sources here and is the same one MCP clients use.
 */
function ServerAccess() {
  const server = useSettings((s) => s.server)
  const saved = useSettings((s) => s.ownerToken)
  const save = useSettings((s) => s.saveOwnerToken)
  const forget = useSettings((s) => s.forgetOwnerToken)
  const [token, setToken] = useState('')
  const [show, setShow] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()

  if (!server?.tokenRequired) return null

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)
    const err = await save(token)
    setBusy(false)
    setError(err)
    if (!err) setToken('')
  }

  return (
    <Card title="Server access" icon={LockKeyhole}>
      {server.owner ? (
        <div className="flex items-center gap-3">
          <p className="flex-1 text-sm text-slate-600 dark:text-slate-300">
            This device has the server’s access token, so your custom sources work here and are kept on the server for MCP clients.
          </p>
          {saved && (
            <button type="button" className={`${buttonClass.secondary} text-sm`} onClick={() => void forget()}>
              Forget token
            </button>
          )}
        </div>
      ) : (
        <form onSubmit={onSubmit} className="space-y-3">
          <p className="text-sm text-slate-500 dark:text-slate-400">
            This server is shared, so custom sources (and the API keys they use) need its access token. The forecast works without it.
          </p>
          {server.tokenRejected && <ErrorNote message="The saved access token no longer works. Enter the current one." />}
          <div className="flex gap-2">
            <label className="relative min-w-0 flex-1">
              <span className="sr-only">Access token</span>
              <input
                type={show ? 'text' : 'password'}
                className={`${inputClass} pr-10 font-mono`}
                placeholder="Access token"
                autoComplete="off"
                spellCheck={false}
                value={token}
                onChange={(e) => setToken(e.target.value)}
              />
              <button
                type="button"
                className="absolute inset-y-0 right-0 px-3 text-slate-400 hover:text-slate-600"
                aria-label={show ? 'Hide token' : 'Show token'}
                onClick={() => setShow(!show)}
              >
                {show ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
              </button>
            </label>
            <button type="submit" className={buttonClass.primary} disabled={busy || token.trim().length === 0}>
              {busy ? 'Checking…' : 'Save'}
            </button>
          </div>
          {error && <ErrorNote message={error} />}
        </form>
      )}
    </Card>
  )
}

const CAP_LABEL: Record<Capability, string> = {
  current: 'Current',
  hourly: 'Hourly',
  daily: 'Daily',
  alerts: 'Alerts',
  marine: 'Marine',
}

function DataSource() {
  const providers = useSettings((s) => s.serverProviders)
  const active = useSettings((s) => s.activeProviderId)
  const setActive = useSettings((s) => s.setActiveProvider)
  const selectable = providers.filter((p) => p.capabilities.includes('current'))
  const alertSources = providers.filter((p) => p.capabilities.includes('alerts'))

  return (
    <Card title="Data source" icon={Server}>
      <p className="mb-3 text-sm text-slate-500 dark:text-slate-400">
        Used for current conditions and forecasts. Anything a source doesn’t cover (such as tides and waves) comes from Open-Meteo.
      </p>
      <div className="space-y-2" role="radiogroup" aria-label="Data source">
        {selectable.map((p) => (
          <label
            key={p.id}
            className={`flex cursor-pointer items-start gap-3 rounded-xl border p-3 ${
              p.id === active
                ? 'border-sky-500 bg-sky-50 dark:border-sky-600 dark:bg-sky-950/40'
                : 'border-slate-200 hover:border-slate-300 dark:border-slate-700'
            }`}
          >
            <input
              type="radio"
              name="provider"
              className="mt-1 accent-sky-600"
              aria-label={p.name}
              checked={p.id === active}
              onChange={() => void setActive(p.id)}
            />
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-medium">
                {p.name}
                {p.id === DEFAULT_PROVIDER && <span className="ml-2 text-xs font-normal text-slate-500">default</span>}
              </span>
              {p.description && (
                <span className="mt-0.5 block text-xs text-slate-500 dark:text-slate-400">{p.description}</span>
              )}
              <span className="mt-1.5 flex flex-wrap gap-1">
                {p.capabilities.map((c) => (
                  <span key={c} className="rounded-md bg-slate-100 px-1.5 py-0.5 text-[11px] text-slate-600 dark:bg-slate-800 dark:text-slate-300">
                    {CAP_LABEL[c]}
                  </span>
                ))}
              </span>
            </span>
          </label>
        ))}
        {!selectable.some((p) => p.id === active) && providers.length > 0 && (
          <ErrorNote message={`The selected source “${active}” isn’t available. Pick another one.`} />
        )}
      </div>
      {alertSources.length > 0 && (
        <>
          <h3 className="mt-5 mb-2 text-xs font-semibold tracking-wide text-slate-500 uppercase dark:text-slate-400">
            Severe weather alerts
          </h3>
          <p className="mb-2 text-sm text-slate-500 dark:text-slate-400">Always from the official service for the place, whichever source you pick.</p>
          <ul className="space-y-2">
            {alertSources.map((p) => (
              <li key={p.id} className="rounded-xl border border-slate-200 p-3 dark:border-slate-700">
                <span className="block text-sm font-medium">{p.name}</span>
                {p.description && <span className="mt-0.5 block text-xs text-slate-500 dark:text-slate-400">{p.description}</span>}
              </li>
            ))}
          </ul>
        </>
      )}
    </Card>
  )
}

function Appearance() {
  const weatherThemes = useSettings((s) => s.weatherThemes)
  const animations = useSettings((s) => s.animations)
  const setWeatherThemes = useSettings((s) => s.setWeatherThemes)
  const setAnimations = useSettings((s) => s.setAnimations)
  return (
    <Card title="Appearance" icon={Palette}>
      <div className="divide-y divide-slate-200 dark:divide-slate-800">
        <Toggle
          label="Weather themes"
          detail="Colours follow the weather: warm on sunny days, grey-blue when it rains, starry at night."
          checked={weatherThemes}
          onChange={(on) => void setWeatherThemes(on)}
        />
        <Toggle
          label="Animations"
          detail="Gentle motion such as drifting clouds, falling rain and a glowing sun. Starts off when your device asks for reduced motion."
          checked={animations}
          onChange={(on) => void setAnimations(on)}
        />
      </div>
    </Card>
  )
}

function Toggle({
  label,
  detail,
  checked,
  onChange,
}: {
  label: string
  detail: string
  checked: boolean
  onChange: (on: boolean) => void
}) {
  return (
    <label className="flex cursor-pointer items-start justify-between gap-4 py-3 first:pt-0 last:pb-0">
      <span>
        <span className="block text-sm font-medium">{label}</span>
        <span className="block text-xs text-slate-500 dark:text-slate-400">{detail}</span>
      </span>
      <span className="relative mt-0.5 inline-flex shrink-0">
        <input
          type="checkbox"
          role="switch"
          className="peer sr-only"
          checked={checked}
          onChange={(e) => onChange(e.target.checked)}
        />
        <span className="h-6 w-11 rounded-full bg-slate-300 transition-colors peer-checked:bg-sky-600 peer-focus-visible:ring-2 peer-focus-visible:ring-sky-500/50 dark:bg-slate-700 dark:peer-checked:bg-sky-500" />
        <span className="absolute top-0.5 left-0.5 size-5 rounded-full bg-white shadow transition-transform peer-checked:translate-x-5" />
      </span>
    </label>
  )
}

function UnitsSection() {
  const units = useSettings((s) => s.units)
  const setUnits = useSettings((s) => s.setUnits)
  const options: { value: Units; label: string; detail: string }[] = [
    { value: 'metric', label: 'Metric', detail: '°C · km/h · mm · hPa' },
    { value: 'imperial', label: 'Imperial', detail: '°F · mph · in · inHg' },
  ]
  return (
    <Card title="Units" icon={Ruler}>
      <div className="grid grid-cols-2 gap-2" role="radiogroup" aria-label="Units">
        {options.map((o) => (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={units === o.value}
            onClick={() => void setUnits(o.value)}
            className={`rounded-xl border p-3 text-left ${
              units === o.value
                ? 'border-sky-500 bg-sky-50 dark:border-sky-600 dark:bg-sky-950/40'
                : 'border-slate-200 hover:border-slate-300 dark:border-slate-700'
            }`}
          >
            <span className="block text-sm font-medium">{o.label}</span>
            <span className="block text-xs text-slate-500">{o.detail}</span>
          </button>
        ))}
      </div>
    </Card>
  )
}

function Locations() {
  const locations = useSettings((s) => s.locations)
  const remove = useSettings((s) => s.removeLocation)
  const [adding, setAdding] = useState(false)
  return (
    <Card
      title="Saved locations"
      icon={MapPin}
      action={
        <button type="button" className={`${buttonClass.ghost} text-sm`} onClick={() => setAdding(true)}>
          <Plus className="size-4" /> Add
        </button>
      }
    >
      {locations.length === 0 ? (
        <p className="text-sm text-slate-500">No saved locations yet.</p>
      ) : (
        <ul className="divide-y divide-slate-100 dark:divide-slate-800">
          {locations.map((l) => (
            <li key={l.id} className="flex items-center gap-3 py-2">
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium">
                  {placeLabel(l)}
                  {l.kind === 'device' && <span className="ml-2 text-xs font-normal text-sky-600 dark:text-sky-400">your location</span>}
                </span>
                <span className="block text-xs text-slate-500">
                  {l.lat.toFixed(3)}, {l.lon.toFixed(3)}
                  {l.kind === 'device' && <> · name © OpenStreetMap contributors</>}
                </span>
              </span>
              <button type="button" className={buttonClass.danger} aria-label={`Remove ${l.name}`} onClick={() => void remove(l.id)}>
                <Trash2 className="size-4" />
              </button>
            </li>
          ))}
        </ul>
      )}
      {adding && <AddLocationDialog onClose={() => setAdding(false)} />}
    </Card>
  )
}

function CustomSources() {
  const providers = useSettings((s) => s.customProviders)
  const keys = useSettings((s) => s.apiKeys)
  const remove = useSettings((s) => s.removeCustomProvider)
  const [editing, setEditing] = useState<StoredCustomProvider | 'new' | null>(null)
  const locked = useSettings((s) => s.server?.tokenRequired && !s.server.owner)

  return (
    <Card
      title="Custom sources"
      icon={Waypoints}
      action={
        !editing && (
          <button type="button" className={`${buttonClass.ghost} text-sm`} onClick={() => setEditing('new')}>
            <Plus className="size-4" /> Add
          </button>
        )
      }
    >
      {locked && !editing && (
        <p className="mb-3 text-sm text-amber-700 dark:text-amber-400">Enter the server’s access token above to use custom sources.</p>
      )}
      {editing ? (
        <CustomProviderEditor existing={editing === 'new' ? undefined : editing} onDone={() => setEditing(null)} />
      ) : providers.length === 0 ? (
        <p className="text-sm text-slate-500 dark:text-slate-400">
          Connect any weather API that returns JSON by describing where its values are. Custom sources provide current conditions.
        </p>
      ) : (
        <ul className="divide-y divide-slate-100 dark:divide-slate-800">
          {providers.map((p) => {
            const key = keys.find((k) => k.id === p.apiKeyId)
            return (
              <li key={p.config.slug} className="flex items-center gap-3 py-2">
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-medium">{p.config.name}</span>
                  <span className="block truncate font-mono text-xs text-slate-500">
                    custom:{p.config.slug}
                    {key && <span className="font-sans"> · key “{key.label}”</span>}
                  </span>
                </span>
                <button type="button" className={buttonClass.ghost} aria-label={`Edit ${p.config.name}`} onClick={() => setEditing(p)}>
                  <Pencil className="size-4" />
                </button>
                <button
                  type="button"
                  className={buttonClass.danger}
                  aria-label={`Delete ${p.config.name}`}
                  onClick={() => confirm(`Delete “${p.config.name}”?`) && void remove(p.config.slug)}
                >
                  <Trash2 className="size-4" />
                </button>
              </li>
            )
          })}
        </ul>
      )}
    </Card>
  )
}

function ApiKeys() {
  const keys = useSettings((s) => s.apiKeys)
  const providers = useSettings((s) => s.customProviders)
  const add = useSettings((s) => s.addApiKey)
  const remove = useSettings((s) => s.removeApiKey)
  const [label, setLabel] = useState('')
  const [secret, setSecret] = useState('')
  const [show, setShow] = useState(false)

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault()
    if (!label.trim() || !secret.trim()) return
    await add(label, secret)
    setLabel('')
    setSecret('')
    setShow(false)
  }

  return (
    <Card title="API keys" icon={KeyRound}>
      <p className="mb-3 flex items-start gap-2 text-xs text-slate-500 dark:text-slate-400">
        <Database className="mt-0.5 size-3.5 shrink-0" aria-hidden />
        Keys are stored unencrypted in this browser’s local database, and copied to your local weather server so AI agents (MCP) can
        use custom sources while the app is closed. They never leave this computer except in requests to the API they belong to.
      </p>
      {keys.length > 0 && (
        <ul className="mb-4 divide-y divide-slate-100 dark:divide-slate-800">
          {keys.map((k) => {
            const usedBy = providers.filter((p) => p.apiKeyId === k.id).map((p) => p.config.name)
            return (
              <li key={k.id} className="flex items-center gap-3 py-2">
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-medium">{k.label}</span>
                  <span className="block text-xs text-slate-500">
                    <span className="font-mono">••••{k.hint}</span>
                    {usedBy.length > 0 && <> · used by {usedBy.join(', ')}</>}
                  </span>
                </span>
                <button
                  type="button"
                  className={buttonClass.danger}
                  aria-label={`Delete key ${k.label}`}
                  onClick={() =>
                    confirm(
                      usedBy.length
                        ? `Delete “${k.label}”? ${usedBy.join(', ')} will stop working until you pick another key.`
                        : `Delete “${k.label}”?`,
                    ) && void remove(k.id)
                  }
                >
                  <Trash2 className="size-4" />
                </button>
              </li>
            )
          })}
        </ul>
      )}
      <form onSubmit={onSubmit} className="grid gap-2 sm:grid-cols-[1fr_1.5fr_auto]">
        <input className={inputClass} value={label} onChange={(e) => setLabel(e.target.value)} placeholder="Label" aria-label="Key label" />
        <div className="relative">
          <input
            className={`${inputClass} pr-9 font-mono`}
            type={show ? 'text' : 'password'}
            value={secret}
            onChange={(e) => setSecret(e.target.value)}
            placeholder="Key"
            aria-label="Key value"
            autoComplete="off"
            spellCheck={false}
          />
          <button
            type="button"
            className="absolute top-1/2 right-2 -translate-y-1/2 p-1 text-slate-400 hover:text-slate-600"
            onClick={() => setShow((s) => !s)}
            aria-label={show ? 'Hide key' : 'Show key'}
          >
            {show ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
          </button>
        </div>
        <button type="submit" className={buttonClass.primary} disabled={!label.trim() || !secret.trim()}>
          <Plus className="size-4" /> Add key
        </button>
      </form>
    </Card>
  )
}
