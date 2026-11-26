import { useEffect, type CSSProperties } from 'react'
import { useSettings } from '../store/settings'
import { useWeather } from '../store/weather'
import { PLAIN, THEMES, moodFor, type Mood } from '../lib/mood'

/** The current mood, or null while there's no weather or themes are off. */
export function useMood(): Mood | null {
  const themes = useSettings((s) => s.weatherThemes)
  const c = useWeather((s) => s.current.data)
  if (!themes || !c) return null
  return moodFor(c.condition, c.isDay)
}

/**
 * The page behind the cards: a soft tint in the weather's colours. Also sets
 * the phone's status-bar colour and the `data-motion` switch the CSS uses.
 */
export function Backdrop() {
  const mood = useMood()
  const animations = useSettings((s) => s.animations)
  const isDay = useWeather((s) => s.current.data?.isDay ?? true)
  const bar = mood ? THEMES[mood].bar : PLAIN[isDay ? 'day' : 'night'].bar

  useEffect(() => {
    document.documentElement.dataset.motion = animations ? 'on' : 'off'
  }, [animations])

  useEffect(() => {
    document.querySelector('meta[name="theme-color"]')?.setAttribute('content', bar)
  }, [bar])

  return (
    <div aria-hidden className="pointer-events-none fixed inset-0 -z-10 overflow-hidden">
      {Object.entries(THEMES).map(([m, t]) => (
        // All washes are rendered so a change of weather cross-fades.
        <div
          key={m}
          className={`absolute inset-x-0 top-0 h-[85vh] bg-gradient-to-b to-transparent transition-opacity duration-1000 ${t.wash} ${
            m === mood ? 'opacity-100' : 'opacity-0'
          }`}
        />
      ))}
      {(mood === 'sunny' || mood === 'partly') && (
        <div className="wx-breathe absolute -top-48 -right-32 size-[36rem] rounded-full bg-amber-300/30 blur-3xl dark:bg-amber-400/10" />
      )}
    </div>
  )
}

/** Effects inside the current-conditions card. Decorative only. */
export function HeroEffects({ mood }: { mood: Mood }) {
  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
      {(mood === 'sunny' || mood === 'partly') && <Sun dim={mood === 'partly'} />}
      {mood === 'night' && <Stars />}
      {(mood === 'partly' || mood === 'cloudy' || mood === 'rain' || mood === 'storm' || mood === 'snow') && (
        <Clouds count={mood === 'partly' ? 2 : 3} dark={mood === 'rain' || mood === 'storm'} />
      )}
      {mood === 'fog' && <Fog />}
      {mood === 'rain' && <Rain drops={36} />}
      {mood === 'storm' && (
        <>
          <Rain drops={48} heavy />
          <div className="wx-flash absolute inset-0 bg-white" />
        </>
      )}
      {mood === 'snow' && <Snow />}
    </div>
  )
}

// Repeatable "random" numbers, so the layout doesn't jump on every render.
function rand(i: number, salt: number): number {
  const x = Math.sin(i * 12.9898 + salt * 78.233) * 43758.5453
  return x - Math.floor(x)
}

const range = (n: number) => Array.from({ length: n }, (_, i) => i)

/** A warm glow in the corner and a few specks of light drifting up, like dust in sunlight. */
function Sun({ dim }: { dim: boolean }) {
  return (
    <div className={dim ? 'opacity-60' : ''}>
      <div className="wx-breathe absolute -top-28 -right-28 size-80 rounded-full bg-[radial-gradient(circle,rgb(255_251_235/0.55)_0%,rgb(254_243_199/0.25)_35%,transparent_70%)]" />
      {range(dim ? 6 : 10).map((i) => (
        <span
          key={i}
          className="wx-mote absolute rounded-full bg-yellow-50"
          style={{
            left: `${10 + rand(i, 17) * 85}%`,
            top: `${30 + rand(i, 18) * 65}%`,
            width: 2 + rand(i, 19) * 3,
            height: 2 + rand(i, 19) * 3,
            animationDuration: `${9 + rand(i, 20) * 7}s`,
            animationDelay: `${-rand(i, 21) * 16}s`,
          }}
        />
      ))}
    </div>
  )
}

function Stars() {
  return (
    <>
      {range(28).map((i) => (
        <span
          key={i}
          className="wx-twinkle absolute rounded-full bg-white"
          style={{
            left: `${rand(i, 1) * 100}%`,
            top: `${rand(i, 2) * 70}%`,
            width: rand(i, 3) > 0.8 ? 3 : 2,
            height: rand(i, 3) > 0.8 ? 3 : 2,
            animationDelay: `${-rand(i, 4) * 6}s`,
            animationDuration: `${3 + rand(i, 5) * 4}s`,
          }}
        />
      ))}
    </>
  )
}

function Clouds({ count, dark }: { count: number; dark: boolean }) {
  return (
    <>
      {range(count).map((i) => (
        // The column spans the card, so moving it by a percentage crosses the card.
        <span
          key={i}
          className="wx-drift absolute inset-x-0"
          style={{
            top: `${8 + i * 28}%`,
            animationDuration: `${70 + i * 25}s`,
            animationDelay: `${-(i * 31 + 10)}s`,
          }}
        >
          <span className={`block h-24 w-72 rounded-full blur-2xl ${dark ? 'bg-slate-300/20' : 'bg-white/25'}`} />
        </span>
      ))}
    </>
  )
}

function Fog() {
  return (
    <>
      {range(3).map((i) => (
        <span
          key={i}
          className="wx-mist absolute -inset-x-1/4 h-16 rounded-full bg-white/20 blur-2xl"
          style={{ top: `${20 + i * 25}%`, animationDuration: `${18 + i * 6}s`, animationDelay: `${-i * 5}s` }}
        />
      ))}
    </>
  )
}

/**
 * Each drop is a full-height column with a streak at its bottom; moving the
 * column by its own height keeps the animation to transforms only.
 */
function Rain({ drops, heavy = false }: { drops: number; heavy?: boolean }) {
  return (
    <div className="absolute -inset-x-8 -inset-y-4 rotate-[8deg]">
      {range(drops).map((i) => (
        <span
          key={i}
          className="absolute inset-y-0 w-px"
          style={{ left: `${rand(i, 6) * 100}%` }}
        >
          <span
            className="wx-fall absolute inset-0"
            style={
              {
                '--len': `${(heavy ? 14 : 10) + rand(i, 7) * 10}%`,
                opacity: 0.25 + rand(i, 8) * 0.35,
                animationDuration: `${(heavy ? 0.55 : 0.8) + rand(i, 9) * 0.5}s`,
                animationDelay: `${-rand(i, 10) * 2}s`,
              } as CSSProperties
            }
          />
        </span>
      ))}
    </div>
  )
}

function Snow() {
  return (
    <>
      {range(32).map((i) => {
        const size = 2 + rand(i, 11) * 4
        return (
          <span key={i} className="absolute inset-y-0" style={{ left: `${rand(i, 12) * 100}%` }}>
            <span
              className="wx-snow absolute inset-0"
              style={{ animationDuration: `${7 + rand(i, 13) * 6}s`, animationDelay: `${-rand(i, 14) * 12}s` }}
            >
              <span
                className="wx-sway absolute bottom-0 block rounded-full bg-white/80"
                style={{
                  width: size,
                  height: size,
                  animationDuration: `${2.5 + rand(i, 15) * 2}s`,
                  animationDelay: `${-rand(i, 16) * 3}s`,
                }}
              />
            </span>
          </span>
        )
      })}
    </>
  )
}
