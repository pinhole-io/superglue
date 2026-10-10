import {
  Activity,
  Camera,
  IdCard,
  KeyRound,
  LayoutDashboard,
  Palette,
  Server,
  Users,
  Wallet,
} from 'lucide-react'
import type { ReactNode } from 'react'
import { navigate, type Route } from '../router'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

type NavItem = {
  label: string
  route: Route
  icon: ReactNode
  match: (route: Route) => boolean
}

type NavGroup = {
  label: string
  items: NavItem[]
}

const NAV: NavGroup[] = [
  {
    label: 'Home',
    items: [
      {
        label: 'Overview',
        route: { name: 'overview' },
        icon: <LayoutDashboard />,
        match: (route) => route.name === 'overview',
      },
    ],
  },
  {
    label: 'Access',
    items: [
      {
        label: 'Users',
        route: { name: 'users' },
        icon: <Users />,
        match: (route) => route.name === 'users',
      },
      {
        label: 'Keys',
        route: { name: 'keys' },
        icon: <KeyRound />,
        match: (route) => route.name === 'keys',
      },
      {
        label: 'Budgets',
        route: { name: 'budgets' },
        icon: <Wallet />,
        match: (route) => route.name === 'budgets',
      },
      {
        label: 'Profiles',
        route: { name: 'profiles' },
        icon: <IdCard />,
        match: (route) => route.name === 'profiles',
      },
    ],
  },
  {
    label: 'Observe',
    items: [
      {
        label: 'Usage',
        route: { name: 'usage' },
        icon: <Activity />,
        match: (route) => route.name === 'usage',
      },
      {
        label: 'Capture',
        route: { name: 'capture' },
        icon: <Camera />,
        match: (route) =>
          route.name === 'capture' ||
          route.name === 'capture-records' ||
          route.name === 'capture-detail',
      },
    ],
  },
  {
    label: 'Platform',
    items: [
      {
        label: 'Providers',
        route: { name: 'providers' },
        icon: <Server />,
        match: (route) => route.name === 'providers',
      },
    ],
  },
  {
    label: 'User Interface',
    items: [
      {
        label: 'Style guide',
        route: { name: 'styleguide' },
        icon: <Palette />,
        match: (route) => route.name === 'styleguide',
      },
    ],
  },
]

export function AppShell({
  route,
  onLogout,
  children,
}: {
  route: Route
  onLogout: () => void
  children: ReactNode
}) {
  return (
    <div className="app-shell">
      <aside className="app-sidebar">
        <div className="app-brand">
          <p className="eyebrow">SUPERGLUE</p>
          <h1>Gateway admin</h1>
        </div>
        <nav className="app-nav" aria-label="Gateway admin">
          {NAV.map((group) => (
            <div className="nav-group" key={group.label}>
              <p className="nav-group-label">{group.label}</p>
              <ul>
                {group.items.map((item) => {
                  const active = item.match(route)
                  return (
                    <li key={item.label}>
                      <button
                        type="button"
                        className={cn('nav-link', active && 'active')}
                        aria-current={active ? 'page' : undefined}
                        onClick={() => navigate(item.route)}
                      >
                        {item.icon}
                        <span>{item.label}</span>
                      </button>
                    </li>
                  )
                })}
              </ul>
            </div>
          ))}
        </nav>
        <div className="app-sidebar-footer">
          <Button type="button" variant="outline" className="w-full" onClick={onLogout}>
            Sign out
          </Button>
        </div>
      </aside>
      <div className="app-content">
        <main className="app-main">{children}</main>
      </div>
    </div>
  )
}

export function LinkButton({
  route,
  children,
  className,
}: {
  route: Route
  children: ReactNode
  className?: string
}) {
  return (
    <button type="button" className={cn('link-button', className)} onClick={() => navigate(route)}>
      {children}
    </button>
  )
}
