import type { Condition, CurrentMapping, Transform } from '../types'

export interface Preset {
  label: string
  name: string
  slug: string
  urlTemplate: string
  headers?: Record<string, string>
  needsKey: boolean
  mapping: CurrentMapping
  conditionMap: Record<string, Condition>
}

const f = (path: string, transform: Transform | null = null) => ({ path, transform })

const none: CurrentMapping = {
  temperatureC: null,
  feelsLikeC: null,
  humidityPct: null,
  windSpeedMs: null,
  windGustMs: null,
  windDirDeg: null,
  pressureHpa: null,
  precipMm: null,
  cloudCoverPct: null,
  uvIndex: null,
  condition: null,
  isDay: null,
  observedAt: null,
}

/** Starting points for well-known APIs. Users still supply their own key. */
export const PRESETS: Preset[] = [
  {
    label: 'OpenWeatherMap (current weather)',
    name: 'OpenWeatherMap',
    slug: 'openweathermap',
    urlTemplate: 'https://api.openweathermap.org/data/2.5/weather?lat={lat}&lon={lon}&units=metric&appid={apiKey}',
    needsKey: true,
    mapping: {
      ...none,
      temperatureC: f('main.temp'),
      feelsLikeC: f('main.feels_like'),
      humidityPct: f('main.humidity'),
      pressureHpa: f('main.pressure'),
      windSpeedMs: f('wind.speed'),
      windGustMs: f('wind.gust'),
      windDirDeg: f('wind.deg'),
      cloudCoverPct: f('clouds.all'),
      precipMm: f('rain.1h'),
      condition: f('weather[0].main'),
      observedAt: f('dt'),
    },
    conditionMap: {
      Clear: 'clear',
      Clouds: 'cloudy',
      Drizzle: 'drizzle',
      Rain: 'rain',
      Thunderstorm: 'thunderstorm',
      Snow: 'snow',
      Mist: 'fog',
      Fog: 'fog',
      Haze: 'fog',
    },
  },
  {
    label: 'WeatherAPI.com (current.json)',
    name: 'WeatherAPI.com',
    slug: 'weatherapi',
    urlTemplate: 'https://api.weatherapi.com/v1/current.json?key={apiKey}&q={lat},{lon}',
    needsKey: true,
    mapping: {
      ...none,
      temperatureC: f('current.temp_c'),
      feelsLikeC: f('current.feelslike_c'),
      humidityPct: f('current.humidity'),
      windSpeedMs: f('current.wind_kph', 'kmh_to_ms'),
      windGustMs: f('current.gust_kph', 'kmh_to_ms'),
      windDirDeg: f('current.wind_degree'),
      pressureHpa: f('current.pressure_mb'),
      precipMm: f('current.precip_mm'),
      cloudCoverPct: f('current.cloud'),
      uvIndex: f('current.uv'),
      isDay: f('current.is_day'),
      observedAt: f('current.last_updated_epoch'),
      condition: f('current.condition.code'),
    },
    conditionMap: {
      '1000': 'clear',
      '1003': 'partly_cloudy',
      '1006': 'cloudy',
      '1009': 'cloudy',
      '1030': 'fog',
      '1135': 'fog',
      '1063': 'rain',
      '1150': 'drizzle',
      '1153': 'drizzle',
      '1180': 'rain',
      '1183': 'rain',
      '1189': 'rain',
      '1195': 'rain',
      '1240': 'rain',
      '1243': 'rain',
      '1198': 'freezing_rain',
      '1204': 'sleet',
      '1066': 'snow',
      '1210': 'snow',
      '1213': 'snow',
      '1219': 'snow',
      '1225': 'snow',
      '1087': 'thunderstorm',
      '1273': 'thunderstorm',
      '1276': 'thunderstorm',
    },
  },
  {
    label: 'Open-Meteo (no key, for trying the adapter)',
    name: 'Open-Meteo (custom)',
    slug: 'open-meteo-custom',
    urlTemplate:
      'https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,relative_humidity_2m,wind_speed_10m,weather_code,is_day&wind_speed_unit=ms&timeformat=unixtime',
    needsKey: false,
    mapping: {
      ...none,
      temperatureC: f('current.temperature_2m'),
      humidityPct: f('current.relative_humidity_2m'),
      windSpeedMs: f('current.wind_speed_10m'),
      isDay: f('current.is_day'),
      observedAt: f('current.time'),
      condition: f('current.weather_code'),
    },
    conditionMap: { '0': 'clear', '1': 'clear', '2': 'partly_cloudy', '3': 'cloudy', '45': 'fog', '61': 'rain', '63': 'rain', '65': 'rain', '71': 'snow', '95': 'thunderstorm' },
  },
]

export const EMPTY_MAPPING = none
