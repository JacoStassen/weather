/// <reference lib="webworker" />
// Owns the SQLite database. Runs in a worker because the OPFS storage backend
// (persistent, no special server headers needed) is only available there.

import sqlite3InitModule, { type BindingSpec, type Database } from '@sqlite.org/sqlite-wasm'
import type { WorkerRequest, WorkerResponse } from './protocol'

interface Opened {
  db: Database
  persistent: boolean
}

let opened: Promise<Opened> | undefined

/**
 * Only one tab can hold the OPFS database. The owner keeps this lock for its
 * lifetime, so other tabs find out within LOCK_WAIT_MS instead of waiting for
 * the VFS to give up retrying (several seconds). The short wait covers a
 * reload, where the old page still holds the lock for a moment.
 */
const LOCK_WAIT_MS = 2000

function claimDatabaseLock(): Promise<boolean> {
  if (!navigator.locks) return Promise.resolve(true)
  return new Promise((resolve) => {
    navigator.locks
      .request('weather-app-db', { signal: AbortSignal.timeout(LOCK_WAIT_MS) }, () => {
        resolve(true)
        // Hold until the tab closes.
        return new Promise<void>(() => {})
      })
      .catch(() => resolve(false))
  })
}

type Sqlite3 = Awaited<ReturnType<typeof sqlite3InitModule>>

/**
 * Opens the OPFS pool. On a reload the previous page can drop the lock just
 * before its worker closes the files, so opening may briefly fail: retry.
 *
 * `preserveOnInitFailure` is essential. Its default (false) makes a failed
 * open DELETE the whole pool, which is how a busy file during a reload
 * wiped all settings once.
 */
async function installPool(sqlite3: Sqlite3) {
  let lastErr: unknown
  for (const delay of [0, 150, 400, 1000]) {
    if (delay) await new Promise((r) => setTimeout(r, delay))
    try {
      // forceReinitIfPreviouslyFailed: retry instead of returning the cached
      // failure. Supported at runtime, missing from the bundled typings.
      const opts = { name: 'weather-app', preserveOnInitFailure: true, forceReinitIfPreviouslyFailed: true }
      return await sqlite3.installOpfsSAHPoolVfs(opts as Parameters<typeof sqlite3.installOpfsSAHPoolVfs>[0])
    } catch (err) {
      lastErr = err
    }
  }
  throw lastErr
}

async function open(): Promise<Opened> {
  const sqlite3 = await sqlite3InitModule()
  let result: Opened
  try {
    if (!(await claimDatabaseLock())) throw new Error('the app is open in another tab')
    const pool = await installPool(sqlite3)
    result = { db: new pool.OpfsSAHPoolDb('/weather.sqlite3'), persistent: true }
  } catch (err) {
    // No OPFS (old browser, private window) or another tab holds the database.
    console.warn('[db] persistent storage unavailable, using an in-memory database:', String(err))
    result = { db: new sqlite3.oo1.DB(':memory:'), persistent: false }
  }
  result.db.exec('PRAGMA foreign_keys = ON')
  return result
}

function reply(msg: WorkerResponse) {
  self.postMessage(msg)
}

self.onmessage = async (ev: MessageEvent<WorkerRequest>) => {
  const req = ev.data
  try {
    const { db, persistent } = await (opened ??= open())
    switch (req.kind) {
      case 'open':
        return reply({ id: req.id, ok: true, result: { persistent } })
      case 'query': {
        const rows = db.exec({
          sql: req.sql,
          bind: req.bind as BindingSpec | undefined,
          rowMode: 'object',
          returnValue: 'resultRows',
        })
        return reply({ id: req.id, ok: true, result: rows })
      }
      case 'batch': {
        db.exec('BEGIN')
        try {
          for (const s of req.statements) db.exec({ sql: s.sql, bind: s.bind as BindingSpec | undefined })
          db.exec('COMMIT')
        } catch (err) {
          db.exec('ROLLBACK')
          throw err
        }
        return reply({ id: req.id, ok: true, result: null })
      }
    }
  } catch (err) {
    reply({ id: req.id, ok: false, error: err instanceof Error ? err.message : String(err) })
  }
}
