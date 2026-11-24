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
let queuedReads = 0
let subscriberTimer: number | null = null
let armGeneration = 0

function publish(next: ModelCatalogState) {
  state = next
  for (const listener of listeners) listener()
}

function catalogReady(): boolean {
  return state.ids.length > 0 || state.refreshedAt !== null
}

function armCatalog() {
  armGeneration += 1
  const generation = armGeneration
  if (subscriberTimer === null) {
    subscriberTimer = window.setInterval(() => {
      void enqueue(false)
    }, HOUR_MS)
  }
  queueMicrotask(() => {
    if (generation !== armGeneration) return
    if (listeners.size === 0) return
    if (state.refreshing || queuedReads > 0 || catalogReady()) return
    void enqueue(false)
  })
}

function disarmCatalog() {
  armGeneration += 1
  if (subscriberTimer !== null) {
    window.clearInterval(subscriberTimer)
    subscriberTimer = null
  }
}

function subscribe(listener: () => void): () => void {
  const first = listeners.size === 0
  listeners.add(listener)
  if (first) armCatalog()
  return () => {
    listeners.delete(listener)
    if (listeners.size === 0) disarmCatalog()
  }
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
  if (!refresh && queuedReads > 0) return tail
  queuedReads += 1
  const run = tail.then(() => read(refresh))
  tail = run.then(
    () => {
      queuedReads -= 1
    },
    () => {
      queuedReads -= 1
    },
  )
  return run
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
