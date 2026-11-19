export type SqlParam = string | number | null

export interface Statement {
  sql: string
  bind?: SqlParam[]
}

export type WorkerRequest =
  | { id: number; kind: 'open' }
  | { id: number; kind: 'query'; sql: string; bind?: SqlParam[] }
  | { id: number; kind: 'batch'; statements: Statement[] }

export type WorkerResponse =
  | { id: number; ok: true; result: unknown }
  | { id: number; ok: false; error: string }
