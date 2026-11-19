import type { SqlParam, Statement, WorkerRequest, WorkerResponse } from './protocol'
import { migrate } from './migrations'

type Pending = { resolve: (v: unknown) => void; reject: (e: Error) => void }
/** `Omit` that keeps each member of a union separate. */
type RequestBody = WorkerRequest extends infer R ? (R extends WorkerRequest ? Omit<R, 'id'> : never) : never

/** Promise wrapper around the SQLite worker. */
export class Db {
  private worker = new Worker(new URL('./sqlite.worker.ts', import.meta.url), { type: 'module' })
  private pending = new Map<number, Pending>()
  private nextId = 1

  /** False when the browser couldn't give us persistent storage. */
  persistent = false

  constructor() {
    this.worker.onmessage = (ev: MessageEvent<WorkerResponse>) => {
      const msg = ev.data
      const p = this.pending.get(msg.id)
      if (!p) return
      this.pending.delete(msg.id)
      if (msg.ok) p.resolve(msg.result)
      else p.reject(new Error(msg.error))
    }
    this.worker.onerror = (ev) => {
      const err = new Error(`database worker failed: ${ev.message}`)
      for (const p of this.pending.values()) p.reject(err)
      this.pending.clear()
    }
  }

  private send<T>(req: RequestBody): Promise<T> {
    const id = this.nextId++
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject })
      this.worker.postMessage({ ...req, id })
    })
  }

  async open(): Promise<void> {
    const { persistent } = await this.send<{ persistent: boolean }>({ kind: 'open' })
    this.persistent = persistent
    await migrate(this)
  }

  query<T>(sql: string, bind?: SqlParam[]): Promise<T[]> {
    return this.send<T[]>({ kind: 'query', sql, bind })
  }

  /** Runs statements in one transaction. */
  batch(statements: Statement[]): Promise<void> {
    return this.send<void>({ kind: 'batch', statements })
  }
}

// Kept on globalThis so a dev hot reload of this module reuses the open
// database. A second worker would find the database locked by the first and
// fall back to memory.
const holder = globalThis as { __weatherDb?: Promise<Db> }

export function getDb(): Promise<Db> {
  holder.__weatherDb ??= (async () => {
    const db = new Db()
    await db.open()
    return db
  })()
  return holder.__weatherDb
}
