import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { api, clearMasterKey, getMasterKey, setMasterKey } from './api'
import { AppShell } from './components/AppShell'
import { GatewayBudgetsPage } from './pages/gateway/BudgetsPage'
import { GatewayCapturePage } from './pages/gateway/CapturePage'
import { GatewayCaptureRecordDetailPage } from './pages/gateway/CaptureRecordDetailPage'
import { GatewayCaptureRecordsPage } from './pages/gateway/CaptureRecordsPage'
import { GatewayKeysPage } from './pages/gateway/KeysPage'
import { GatewayOverviewPage } from './pages/gateway/OverviewPage'
import { GatewayProfilesPage } from './pages/gateway/ProfilesPage'
import { GatewayProvidersPage } from './pages/gateway/ProvidersPage'
import { GatewayUsagePage } from './pages/gateway/UsagePage'
import { GatewayUsersPage } from './pages/gateway/UsersPage'
import { SetupWizard } from './pages/SetupWizard'
import { StyleGuidePage } from './pages/StyleGuidePage'
import { currentRoute, navigate, type Route } from './router'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { TooltipProvider } from '@/components/ui/tooltip'
import './style.css'

type BootState = 'loading' | 'ready' | 'setup'

async function isEmptyGateway(): Promise<boolean> {
  const [users, keys, budgets] = await Promise.all([
    api.users(),
    api.keys(),
    api.budgets(),
  ])
  return users.users.length === 0 && keys.keys.length === 0 && budgets.budgets.length === 0
}

function Login({ onLogin }: { onLogin: () => void }) {
  const [key, setKey] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    setError(null)
    setLoading(true)
    setMasterKey(key.trim())
    try {
      await api.users()
      onLogin()
    } catch (cause) {
      clearMasterKey()
      setError(cause instanceof Error ? cause.message : 'The master key was rejected.')
    } finally {
      setLoading(false)
    }
  }

  return (
    <main className="login-shell">
      <form className="login-card" onSubmit={submit}>
        <p className="eyebrow">SUPERGLUE</p>
        <h1>Gateway admin</h1>
        <p className="muted">Use the gateway master key to manage users, keys, budgets, and usage.</p>
        <div className="grid gap-1.5">
          <Label htmlFor="master-key">Master key</Label>
          <Input
            id="master-key"
            autoFocus
            type="password"
            value={key}
            onChange={(event) => setKey(event.target.value)}
            placeholder="GATEWAY_MASTER_KEY"
          />
        </div>
        {error ? <p className="error">{error}</p> : null}
        <Button disabled={!key.trim() || loading} type="submit">
          {loading ? 'Checking…' : 'Sign in'}
        </Button>
      </form>
    </main>
  )
}

function useRoute(): Route {
  const [route, setRoute] = useState<Route>(() => currentRoute())
  useEffect(() => {
    const onPop = () => setRoute(currentRoute())
    window.addEventListener('popstate', onPop)
    return () => window.removeEventListener('popstate', onPop)
  }, [])
  return route
}

function AdminApp({ onLogout }: { onLogout: () => void }) {
  const route = useRoute()

  useEffect(() => {
    if (window.location.pathname === '/admin' || window.location.pathname === '/admin/') {
      navigate({ name: 'overview' }, true)
    }
  }, [])

  let page
  switch (route.name) {
    case 'overview':
      page = <GatewayOverviewPage />
      break
    case 'users':
      page = <GatewayUsersPage />
      break
    case 'keys':
      page = <GatewayKeysPage />
      break
    case 'budgets':
      page = <GatewayBudgetsPage />
      break
    case 'profiles':
      page = <GatewayProfilesPage />
      break
    case 'usage':
      page = <GatewayUsagePage />
      break
    case 'providers':
      page = <GatewayProvidersPage />
      break
    case 'capture':
      page = <GatewayCapturePage />
      break
    case 'capture-records':
      page = <GatewayCaptureRecordsPage />
      break
    case 'capture-detail':
      page = <GatewayCaptureRecordDetailPage requestId={route.requestId} />
      break
    case 'styleguide':
      page = <StyleGuidePage />
      break
  }

  return (
    <AppShell route={route} onLogout={onLogout}>
      {page}
    </AppShell>
  )
}

function AuthenticatedApp({ onLogout }: { onLogout: () => void }) {
  const [boot, setBoot] = useState<BootState>('loading')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setBoot('loading')
    isEmptyGateway()
      .then((empty) => {
        if (!cancelled) setBoot(empty ? 'setup' : 'ready')
      })
      .catch((cause) => {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : String(cause))
          setBoot('ready')
        }
      })
    return () => {
      cancelled = true
    }
  }, [])

  if (boot === 'loading') {
    return (
      <main className="login-shell">
        <div className="login-card">
          <p className="eyebrow">SUPERGLUE</p>
          <h1>Gateway admin</h1>
          <p className="muted">Checking gateway configuration…</p>
        </div>
      </main>
    )
  }

  if (boot === 'setup') {
    return (
      <SetupWizard
        onComplete={() => {
          navigate({ name: 'overview' }, true)
          setBoot('ready')
        }}
      />
    )
  }

  return (
    <>
      {error ? (
        <div className="p-3">
          <p className="error m-0">{error}</p>
        </div>
      ) : null}
      <AdminApp onLogout={onLogout} />
    </>
  )
}

function App() {
  const [authenticated, setAuthenticated] = useState(Boolean(getMasterKey()))
  useEffect(() => {
    if (!authenticated) clearMasterKey()
  }, [authenticated])
  return (
    <TooltipProvider>
      {authenticated ? (
        <AuthenticatedApp onLogout={() => { clearMasterKey(); setAuthenticated(false) }} />
      ) : (
        <Login onLogin={() => setAuthenticated(true)} />
      )}
    </TooltipProvider>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
