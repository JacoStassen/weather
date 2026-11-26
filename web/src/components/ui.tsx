import type { ReactNode } from 'react'
import { LoaderCircle, TriangleAlert, type LucideIcon } from 'lucide-react'

export function Card({
  title,
  icon: Icon,
  action,
  className = '',
  children,
}: {
  title?: string
  icon?: LucideIcon
  action?: ReactNode
  className?: string
  children: ReactNode
}) {
  return (
    <section
      className={`rounded-2xl border border-slate-200/80 bg-white/80 p-4 shadow-sm backdrop-blur-md sm:p-5 dark:border-slate-800/80 dark:bg-slate-900/75 ${className}`}
    >
      {title && (
        <header className="mb-3 flex items-center justify-between gap-2">
          <h2 className="flex items-center gap-2 text-xs font-semibold tracking-wide text-slate-500 uppercase dark:text-slate-400">
            {Icon && <Icon className="size-4" aria-hidden />}
            {title}
          </h2>
          {action}
        </header>
      )}
      {children}
    </section>
  )
}

export function Spinner({ className = 'size-5' }: { className?: string }) {
  return <LoaderCircle className={`animate-spin text-slate-400 ${className}`} aria-label="Loading" />
}

export function ErrorNote({ message, className = '' }: { message: string; className?: string }) {
  return (
    <p className={`flex items-start gap-2 text-sm text-rose-700 dark:text-rose-300 ${className}`} role="alert">
      <TriangleAlert className="mt-0.5 size-4 shrink-0" aria-hidden />
      <span>{message}</span>
    </p>
  )
}

export function Skeleton({ className }: { className: string }) {
  return <div className={`animate-pulse rounded-lg bg-slate-200 dark:bg-slate-800 ${className}`} />
}

export const inputClass =
  'w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 placeholder:text-slate-400 focus:border-sky-500 focus:ring-2 focus:ring-sky-500/30 focus:outline-none dark:border-slate-700 dark:bg-slate-950 dark:text-slate-100'

export const buttonClass = {
  primary:
    'inline-flex items-center justify-center gap-2 rounded-lg bg-sky-600 px-3.5 py-2 text-sm font-medium text-white hover:bg-sky-700 disabled:opacity-50 disabled:pointer-events-none',
  secondary:
    'inline-flex items-center justify-center gap-2 rounded-lg border border-slate-300 bg-white px-3.5 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 disabled:pointer-events-none dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800',
  ghost:
    'inline-flex items-center justify-center gap-2 rounded-lg p-2 text-slate-500 hover:bg-slate-100 hover:text-slate-700 disabled:opacity-50 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-slate-200',
  danger:
    'inline-flex items-center justify-center gap-2 rounded-lg p-2 text-slate-400 hover:bg-rose-50 hover:text-rose-600 dark:hover:bg-rose-950 dark:hover:text-rose-400',
}
