/** History API router for the gateway admin SPA under `/admin`. */

export const BASE = '/admin'

export type Route =
  | { name: 'overview' }
  | { name: 'users' }
  | { name: 'keys' }
  | { name: 'key-edit'; keyId: string }
  | { name: 'budgets' }
  | { name: 'profiles' }
  | { name: 'profile-edit'; profileId: string }
  | { name: 'usage' }
  | { name: 'providers' }
  | { name: 'capture' }
  | { name: 'capture-records' }
  | { name: 'capture-detail'; requestId: string }
  | { name: 'styleguide' }

export function parsePath(pathname: string): Route {
  const raw = pathname.startsWith(BASE) ? pathname.slice(BASE.length) : pathname
  const path = raw.replace(/^\/+|\/+$/g, '')
  if (!path || path === 'overview') return { name: 'overview' }
  if (path === 'users') return { name: 'users' }
  if (path === 'keys') return { name: 'keys' }
  const keyEdit = path.match(/^keys\/([^/]+)$/)
  if (keyEdit) return { name: 'key-edit', keyId: decodeURIComponent(keyEdit[1]) }
  if (path === 'budgets') return { name: 'budgets' }
  if (path === 'profiles') return { name: 'profiles' }
  const profileEdit = path.match(/^profiles\/([^/]+)$/)
  if (profileEdit) return { name: 'profile-edit', profileId: decodeURIComponent(profileEdit[1]) }
  if (path === 'usage') return { name: 'usage' }
  if (path === 'providers') return { name: 'providers' }
  if (path === 'capture') return { name: 'capture' }
  if (path === 'capture/records') return { name: 'capture-records' }
  const detail = path.match(/^capture\/records\/([^/]+)$/)
  if (detail) return { name: 'capture-detail', requestId: decodeURIComponent(detail[1]) }
  if (path === 'styleguide') return { name: 'styleguide' }
  return { name: 'overview' }
}

export function pathFor(route: Route): string {
  switch (route.name) {
    case 'overview':
      return `${BASE}/`
    case 'users':
      return `${BASE}/users`
    case 'keys':
      return `${BASE}/keys`
    case 'key-edit':
      return `${BASE}/keys/${encodeURIComponent(route.keyId)}`
    case 'budgets':
      return `${BASE}/budgets`
    case 'profiles':
      return `${BASE}/profiles`
    case 'profile-edit':
      return `${BASE}/profiles/${encodeURIComponent(route.profileId)}`
    case 'usage':
      return `${BASE}/usage`
    case 'providers':
      return `${BASE}/providers`
    case 'capture':
      return `${BASE}/capture`
    case 'capture-records':
      return `${BASE}/capture/records`
    case 'capture-detail':
      return `${BASE}/capture/records/${encodeURIComponent(route.requestId)}`
    case 'styleguide':
      return `${BASE}/styleguide`
  }
}

export function navigate(route: Route, replace = false): void {
  const url = pathFor(route)
  if (replace) {
    window.history.replaceState(null, '', url)
  } else {
    window.history.pushState(null, '', url)
  }
  window.dispatchEvent(new PopStateEvent('popstate'))
}

export function currentRoute(): Route {
  return parsePath(window.location.pathname)
}
