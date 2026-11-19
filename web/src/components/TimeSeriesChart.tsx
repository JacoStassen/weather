import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react'

export interface SeriesPoint {
  /** Epoch milliseconds. */
  t: number
  v: number
}

export interface ChartMarker extends SeriesPoint {
  label: string
  placement: 'above' | 'below'
}

/**
 * Single-series line chart with a crosshair tooltip (pointer and arrow keys).
 * One series, so no legend: the card title names it. Colour comes from the
 * `colorClass` text colour (validated against both surfaces).
 */
export function TimeSeriesChart({
  points,
  height,
  ariaLabel,
  formatValue,
  formatTick = formatValue,
  formatTime,
  xTicks,
  markers = [],
  now,
  area = false,
  colorClass = 'text-sky-600',
}: {
  points: SeriesPoint[]
  height: number
  ariaLabel: string
  /** Tooltip value. */
  formatValue: (v: number) => string
  /** Y-axis labels; defaults to `formatValue`. */
  formatTick?: (v: number) => string
  formatTime: (t: number) => string
  xTicks: { t: number; label: string }[]
  markers?: ChartMarker[]
  now?: number
  area?: boolean
  colorClass?: string
}) {
  const wrap = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(600)
  const [active, setActive] = useState<number | null>(null)

  useEffect(() => {
    const el = wrap.current
    if (!el) return
    const ro = new ResizeObserver(([entry]) => setWidth(Math.max(240, entry.contentRect.width)))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const m = { top: markers.length ? 24 : 10, right: 10, bottom: 22, left: 40 }
  const plotW = width - m.left - m.right
  const plotH = height - m.top - m.bottom

  const geo = useMemo(() => {
    if (points.length < 2) return null
    const t0 = points[0].t
    const t1 = points[points.length - 1].t
    const vals = [...points.map((p) => p.v), ...markers.map((p) => p.v)]
    let lo = Math.min(...vals)
    let hi = Math.max(...vals)
    const pad = (hi - lo) * 0.12 || 0.5
    lo -= pad
    hi += pad
    const step = niceStep((hi - lo) / 3)
    const ticks: number[] = []
    for (let v = Math.ceil(lo / step) * step; v <= hi + 1e-9; v += step) ticks.push(+v.toFixed(6))
    const x = (t: number) => m.left + ((t - t0) / (t1 - t0)) * plotW
    const y = (v: number) => m.top + (1 - (v - lo) / (hi - lo)) * plotH
    const line = points.map((p, i) => `${i ? 'L' : 'M'}${x(p.t).toFixed(1)},${y(p.v).toFixed(1)}`).join('')
    const fill = `${line}L${x(t1).toFixed(1)},${m.top + plotH}L${x(t0).toFixed(1)},${m.top + plotH}Z`
    return { t0, t1, x, y, ticks, line, fill }
  }, [points, markers, plotW, plotH, m.left, m.top])

  if (!geo) return null

  const nearest = (t: number) => {
    let best = 0
    for (let i = 1; i < points.length; i++) if (Math.abs(points[i].t - t) < Math.abs(points[best].t - t)) best = i
    return best
  }
  const onMove = (e: PointerEvent<SVGRectElement>) => {
    const rect = e.currentTarget.getBoundingClientRect()
    const t = geo.t0 + ((e.clientX - rect.left) / rect.width) * (geo.t1 - geo.t0)
    setActive(nearest(t))
  }
  const onKey = (e: KeyboardEvent) => {
    const start = active ?? nearest(now ?? geo.t0)
    if (e.key === 'ArrowRight') setActive(Math.min(start + (active == null ? 0 : 1), points.length - 1))
    else if (e.key === 'ArrowLeft') setActive(Math.max(start - (active == null ? 0 : 1), 0))
    else if (e.key === 'Escape') setActive(null)
    else return
    e.preventDefault()
  }

  const hover = active != null ? points[active] : null
  const hx = hover ? geo.x(hover.t) : 0

  return (
    <div ref={wrap} className={`relative ${colorClass}`}>
      <svg
        width={width}
        height={height}
        role="img"
        aria-label={ariaLabel}
        tabIndex={0}
        onKeyDown={onKey}
        onBlur={() => setActive(null)}
        className="block overflow-visible rounded-md focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-sky-500"
      >
        {/* Recessive grid and y-axis labels */}
        {geo.ticks.map((v) => (
          <g key={v}>
            <line
              x1={m.left}
              x2={m.left + plotW}
              y1={geo.y(v)}
              y2={geo.y(v)}
              className="stroke-slate-200 dark:stroke-slate-800"
              strokeWidth={1}
            />
            <text x={m.left - 6} y={geo.y(v)} dy="0.32em" textAnchor="end" className="fill-slate-500 text-[10px] dark:fill-slate-400">
              {formatTick(v)}
            </text>
          </g>
        ))}
        {xTicks
          .filter((tk) => tk.t >= geo.t0 && tk.t <= geo.t1)
          .map((tk) => (
            <text
              key={tk.t}
              x={geo.x(tk.t)}
              y={height - 6}
              textAnchor="middle"
              className="fill-slate-500 text-[10px] dark:fill-slate-400"
            >
              {tk.label}
            </text>
          ))}

        {area && <path d={geo.fill} fill="currentColor" fillOpacity={0.1} />}
        <path d={geo.line} fill="none" stroke="currentColor" strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />

        {now != null && now >= geo.t0 && now <= geo.t1 && (
          <g>
            <line
              x1={geo.x(now)}
              x2={geo.x(now)}
              y1={m.top - 4}
              y2={m.top + plotH}
              className="stroke-slate-400 dark:stroke-slate-500"
              strokeWidth={1}
            />
            <text x={geo.x(now) + 4} y={m.top + plotH - 4} className="fill-slate-500 text-[10px] dark:fill-slate-400">
              Now
            </text>
          </g>
        )}

        {markers.map((mk) => {
          const x = Math.min(Math.max(geo.x(mk.t), m.left + 14), m.left + plotW - 14)
          const y = geo.y(mk.v)
          return (
            <g key={mk.t}>
              <circle cx={geo.x(mk.t)} cy={y} r={4} fill="currentColor" strokeWidth={2} className="stroke-white dark:stroke-slate-900" />
              <text
                x={x}
                y={mk.placement === 'above' ? y - 9 : y + 16}
                textAnchor="middle"
                className="fill-slate-600 text-[10px] font-medium dark:fill-slate-300"
              >
                {mk.label}
              </text>
            </g>
          )
        })}

        {hover && (
          <g pointerEvents="none">
            <line x1={hx} x2={hx} y1={m.top} y2={m.top + plotH} className="stroke-slate-400 dark:stroke-slate-500" strokeWidth={1} />
            <circle cx={hx} cy={geo.y(hover.v)} r={4} fill="currentColor" strokeWidth={2} className="stroke-white dark:stroke-slate-900" />
          </g>
        )}

        {/* Hit area: the whole plot, so the pointer only has to find the time. */}
        <rect
          x={m.left}
          y={0}
          width={plotW}
          height={height}
          fill="transparent"
          onPointerMove={onMove}
          onPointerDown={onMove}
          onPointerLeave={() => setActive(null)}
        />
      </svg>

      {hover && (
        <div
          className="pointer-events-none absolute top-0 z-10 rounded-lg border border-slate-200 bg-white px-2.5 py-1.5 text-xs shadow-md dark:border-slate-700 dark:bg-slate-900"
          style={{ left: Math.min(Math.max(hx, 60), width - 60), transform: 'translate(-50%, -100%)' }}
          role="status"
        >
          <span className="block font-semibold text-slate-900 dark:text-slate-100">{formatValue(hover.v)}</span>
          <span className="block text-slate-500 dark:text-slate-400">{formatTime(hover.t)}</span>
        </div>
      )}
    </div>
  )
}

/** 1, 2, 2.5 or 5 × 10ⁿ, at least `raw`. */
function niceStep(raw: number): number {
  const p = 10 ** Math.floor(Math.log10(raw))
  for (const k of [1, 2, 2.5, 5, 10]) if (k * p >= raw) return k * p
  return 10 * p
}
