import { useSyncExternalStore } from 'react'
import { api } from './api'

/** Matches the gateway upstream refresh interval. */
const HOUR_MS = 60 * 60 * 1000

export type ModelCatalogState = {
  ids: string[]
  refreshedAt: number | null
  refreshing: boolean
  error: string | null
}

const empty: ModelCatalogState = {
  ids: [],
  refreshedAt: null,
  refreshing: false,
  error: null,
}

let state: ModelCatalogState = empty
const listeners = new Set<() => void>()
let tail: Promise<void> = Promise.resolve()

function publish(next: ModelCatalogState) {
  state = next
  for (const listener of listeners) listener()
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

function snapshot(): ModelCatalogState {
  return state
}

export function useModelCatalog(): ModelCatalogState {
  return useSyncExternalStore(subscribe, snapshot, snapshot)
}

async function read(refresh: boolean): Promise<void> {
  publish({ ...state, refreshing: true, error: null })
  try {
    const result = refresh ? await api.refreshModels() : await api.models()
    publish({
      ids: result.data.map((model) => model.id),
      refreshedAt: result.refreshed_at,
      refreshing: false,
      error: null,
    })
  } catch (cause) {
    publish({
      ...state,
      refreshing: false,
      error: cause instanceof Error ? cause.message : 'The model list failed to load.',
    })
  }
}

function enqueue(refresh: boolean): Promise<void> {
  const run = tail.then(() => read(refresh))
  tail = run.then(
    () => undefined,
    () => undefined,
  )
  return run
}

/** Load the cached catalog, then re-read it every hour. */
export function startModelCatalog(): () => void {
  const timer = window.setInterval(() => {
    void enqueue(false)
  }, HOUR_MS)
  void enqueue(false)
  return () => window.clearInterval(timer)
}

export function refreshModelCatalog(): Promise<void> {
  return enqueue(true)
}

export function catalogStatus(catalog: ModelCatalogState, now: number): string {
  if (catalog.error) return catalog.error
  if (catalog.refreshing && catalog.ids.length === 0) return 'Loading models…'
  if (catalog.refreshing) return 'Refreshing models…'
  if (catalog.refreshedAt === null) return 'The gateway has not refreshed this list yet.'
  const minutes = Math.max(0, Math.floor((now - catalog.refreshedAt) / 60000))
  if (minutes < 1) return 'Updated just now. The gateway refreshes this list every hour.'
  if (minutes === 1) return 'Updated 1 minute ago. The gateway refreshes this list every hour.'
  if (minutes < 60) {
    return `Updated ${minutes} minutes ago. The gateway refreshes this list every hour.`
  }
  const hours = Math.floor(minutes / 60)
  if (hours === 1) return 'Updated 1 hour ago. The gateway refreshes this list every hour.'
  return `Updated ${hours} hours ago. The gateway refreshes this list every hour.`
}
