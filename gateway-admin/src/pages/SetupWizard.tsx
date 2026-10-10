import { useState } from 'react'
import { api } from '../api'
import { InfoTip } from '../components/InfoTip'
import type { CreateKeyResponse } from '../types'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

const MONTH_SEC = 2_592_000
const DEFAULT_MODELS = ['openai:*', 'anthropic:*']

type Step = 'welcome' | 'budget' | 'profile' | 'user' | 'key' | 'done'

const STEPS: Step[] = ['welcome', 'budget', 'profile', 'user', 'key', 'done']

export function SetupWizard({
  onComplete,
}: {
  onComplete: () => void
}) {
  const [step, setStep] = useState<Step>('welcome')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [budget, setBudget] = useState({ max_budget: 10, duration_sec: MONTH_SEC, enforce: true })
  const [profileName, setProfileName] = useState('default')
  const [user, setUser] = useState({ user_id: 'default', alias: 'Default' })
  const [keyName, setKeyName] = useState('default-app')
  const [budgetId, setBudgetId] = useState<string | null>(null)
  const [profileId, setProfileId] = useState<string | null>(null)
  const [plaintext, setPlaintext] = useState<CreateKeyResponse | null>(null)

  const stepIndex = STEPS.indexOf(step)

  const createBudget = async () => {
    setBusy(true)
    setError(null)
    try {
      const created = await api.createBudget(budget)
      setBudgetId(created.id)
      setStep('profile')
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }

  const createProfile = async () => {
    if (!budgetId) {
      setError('Budget is missing. Go back one step.')
      return
    }
    setBusy(true)
    setError(null)
    try {
      const created = await api.createProfile({
        name: profileName.trim() || 'default',
        allowed_models: [...DEFAULT_MODELS],
        budget_id: budgetId,
        enabled: true,
      })
      setProfileId(created.id)
      setStep('user')
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }

  const createUser = async () => {
    if (!profileId) {
      setError('Profile is missing. Go back one step.')
      return
    }
    setBusy(true)
    setError(null)
    try {
      await api.createUser({
        user_id: user.user_id.trim(),
        alias: user.alias.trim() || undefined,
        profile_id: profileId,
      })
      setStep('key')
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }

  const createKey = async () => {
    setBusy(true)
    setError(null)
    try {
      const created = await api.createKey({
        user_id: user.user_id.trim(),
        name: keyName.trim() || undefined,
      })
      setPlaintext(created)
      setStep('done')
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }

  return (
    <main className="login-shell">
      <div className="setup-card">
        <p className="eyebrow">SUPERGLUE</p>
        <h1>First-run setup</h1>
        <p className="muted">
          This gateway has no users, keys, or budgets. Create a default system to start.
        </p>

        <ol className="setup-steps" aria-label="Setup progress">
          {(['Welcome', 'Budget', 'Profile', 'User', 'Key', 'Done'] as const).map((label, index) => (
            <li key={label} className={index === stepIndex ? 'active' : index < stepIndex ? 'done' : ''}>
              {label}
            </li>
          ))}
        </ol>

        {error ? (
          <Alert variant="destructive">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}

        {step === 'welcome' && (
          <div className="grid gap-3">
            <p className="m-0 text-sm">
              The wizard creates one budget, one profile (`openai:*`, `anthropic:*`), one user on that
              profile, and one key that inherits the profile models.
            </p>
            <div className="setup-actions">
              <Button type="button" onClick={() => setStep('budget')}>Start setup</Button>
            </div>
          </div>
        )}

        {step === 'budget' && (
          <div className="grid gap-3">
            <div className="grid gap-1.5">
              <Label htmlFor="setup-budget-max">Max budget (USD)</Label>
              <Input
                id="setup-budget-max"
                type="number"
                min={0}
                step={0.01}
                value={budget.max_budget}
                onChange={(e) => setBudget({ ...budget, max_budget: Number(e.target.value) })}
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="setup-budget-days">Duration (days)</Label>
              <Input
                id="setup-budget-days"
                type="number"
                min={1}
                value={Math.round(budget.duration_sec / 86_400)}
                onChange={(e) =>
                  setBudget({ ...budget, duration_sec: Math.max(1, Number(e.target.value)) * 86_400 })
                }
              />
            </div>
            <label className="check-row">
              <Checkbox
                checked={budget.enforce}
                onCheckedChange={(value) => setBudget({ ...budget, enforce: value === true })}
              />
              <span className="inline-flex items-center gap-1">
                Enforce
                <InfoTip label="Enforce">
                  Enforced budgets reject requests at the limit. Track-only budgets record spend and still allow the call.
                </InfoTip>
              </span>
            </label>
            <div className="setup-actions">
              <Button type="button" variant="outline" onClick={() => setStep('welcome')}>Back</Button>
              <Button type="button" disabled={busy || !(budget.max_budget > 0)} onClick={createBudget}>
                {busy ? 'Creating…' : 'Create budget'}
              </Button>
            </div>
          </div>
        )}

        {step === 'profile' && (
          <div className="grid gap-3">
            <div className="grid gap-1.5">
              <Label htmlFor="setup-profile-name">Profile name</Label>
              <Input
                id="setup-profile-name"
                value={profileName}
                onChange={(e) => setProfileName(e.target.value)}
                placeholder="default"
              />
            </div>
            <p className="muted m-0">
              Models: {DEFAULT_MODELS.join(', ')}. Budget links to the tier you just created.
            </p>
            <div className="setup-actions">
              <Button type="button" variant="outline" onClick={() => setStep('budget')}>Back</Button>
              <Button type="button" disabled={busy || !profileName.trim()} onClick={createProfile}>
                {busy ? 'Creating…' : 'Create profile'}
              </Button>
            </div>
          </div>
        )}

        {step === 'user' && (
          <div className="grid gap-3">
            <div className="grid gap-1.5">
              <Label htmlFor="setup-user-id">User ID</Label>
              <Input
                id="setup-user-id"
                value={user.user_id}
                onChange={(e) => setUser({ ...user, user_id: e.target.value })}
                placeholder="default"
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="setup-user-alias">Alias</Label>
              <Input
                id="setup-user-alias"
                value={user.alias}
                onChange={(e) => setUser({ ...user, alias: e.target.value })}
                placeholder="Default"
              />
            </div>
            <p className="muted m-0">This user is assigned the profile you just created (budget included).</p>
            <div className="setup-actions">
              <Button type="button" variant="outline" onClick={() => setStep('profile')}>Back</Button>
              <Button type="button" disabled={busy || !user.user_id.trim()} onClick={createUser}>
                {busy ? 'Creating…' : 'Create user'}
              </Button>
            </div>
          </div>
        )}

        {step === 'key' && (
          <div className="grid gap-3">
            <div className="grid gap-1.5">
              <Label htmlFor="setup-key-name">Key name</Label>
              <Input
                id="setup-key-name"
                value={keyName}
                onChange={(e) => setKeyName(e.target.value)}
                placeholder="default-app"
              />
            </div>
            <p className="muted m-0">
              Models come from the user profile ({DEFAULT_MODELS.join(', ')}).
            </p>
            <div className="setup-actions">
              <Button type="button" variant="outline" onClick={() => setStep('user')}>Back</Button>
              <Button type="button" disabled={busy} onClick={createKey}>
                {busy ? 'Creating…' : 'Create key'}
              </Button>
            </div>
          </div>
        )}

        {step === 'done' && plaintext && (
          <div className="grid gap-3">
            <Alert>
              <AlertDescription>
                <span className="inline-flex items-center gap-1">
                  Copy the virtual key now. The gateway shows it once.
                  <InfoTip label="One-time secret">
                    The plaintext key is shown once. Store it now. The gateway keeps only a hash.
                  </InfoTip>
                </span>
              </AlertDescription>
            </Alert>
            <pre className="setup-secret">{plaintext.key}</pre>
            <p className="muted m-0">
              {`User ${user.user_id} · key ${plaintext.key_prefix} · models ${(plaintext.allowed_models ?? DEFAULT_MODELS).join(', ')}`}
            </p>
            <div className="setup-actions">
              <Button
                type="button"
                variant="outline"
                onClick={() => void navigator.clipboard.writeText(plaintext.key)}
              >
                Copy key
              </Button>
              <Button type="button" onClick={onComplete}>Open admin</Button>
            </div>
          </div>
        )}
      </div>
    </main>
  )
}
