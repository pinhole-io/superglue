import { useEffect, useState } from 'react'
import { api } from '../../api'
import { routeSummary, shortId } from '../../components/compactDisplay'
import { ChipRow, RowActions } from '../../components/compactUi'
import { InfoTip } from '../../components/InfoTip'
import { ModelPicker } from '../../components/ModelPicker'
import { useModelCatalog } from '../../modelCatalog'
import { PageHeader } from '../../components/PageHeader'
import { PaginatedTable } from '../../components/PaginatedTable'
import type { GatewayBudget, GatewayProfile, ProfileRouteStep } from '../../types'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { errorText } from '@/lib/utils'

const REASONING_LEVELS = ['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'] as const
const MAX_ROUTE_STEPS = 16

type RouteDraft = {
  model: string
  untilUsd: string
  reasoning: string
}

type ProfileForm = {
  name: string
  description: string
  budget_id: string
  max_reasoning_effort: string
  enabled: boolean
  models: string[]
  useDefault: boolean
  routes: RouteDraft[]
}

const emptyForm = (): ProfileForm => ({
  name: '',
  description: '',
  budget_id: '',
  max_reasoning_effort: '',
  enabled: true,
  models: [],
  useDefault: true,
  routes: [],
})

function draftsFromRoutes(routes: ProfileRouteStep[] | undefined): RouteDraft[] {
  return (routes ?? []).map((step) => ({
    model: step.model,
    untilUsd: step.until_usd == null ? '' : String(step.until_usd),
    reasoning: step.reasoning_effort ?? '',
  }))
}

function routeError(routes: RouteDraft[]): string | null {
  if (routes.length > MAX_ROUTE_STEPS) return `A route can have at most ${MAX_ROUTE_STEPS} steps.`
  let previous = 0
  for (let index = 0; index < routes.length; index += 1) {
    const step = routes[index]
    const model = step.model.trim()
    const provider = model.split(':')[0] ?? ''
    const rest = model.slice(provider.length + 1)
    if (!provider || !rest || model.includes('*')) {
      return `Step ${index + 1} needs an exact provider:model.`
    }
    const capText = step.untilUsd.trim()
    const last = index + 1 === routes.length
    if (!capText) {
      if (!last) return 'Only the last step can omit the spend cap.'
      continue
    }
    const cap = Number(capText)
    if (!Number.isFinite(cap) || cap <= 0) return `Step ${index + 1} needs a spend cap greater than 0.`
    if (cap <= previous) return 'Spend caps must increase.'
    previous = cap
  }
  return null
}

function stepsFromDrafts(routes: RouteDraft[]): ProfileRouteStep[] {
  return routes.map((step, index) => {
    const last = index + 1 === routes.length
    const capText = step.untilUsd.trim()
    return {
      model: step.model.trim(),
      ...(!last || capText ? { until_usd: Number(capText) } : {}),
      ...(step.reasoning ? { reasoning_effort: step.reasoning } : {}),
    }
  })
}

function formReady(form: ProfileForm): boolean {
  if (!form.name.trim() || routeError(form.routes)) return false
  const models = form.useDefault ? ['openai:*', 'anthropic:*'] : form.models
  return models.length > 0 || form.routes.length > 0
}

export function GatewayProfilesPage() {
  const [profiles, setProfiles] = useState<GatewayProfile[]>([])
  const [budgets, setBudgets] = useState<GatewayBudget[]>([])
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState<ProfileForm>(emptyForm)
  const [editing, setEditing] = useState<GatewayProfile | null>(null)
  const [editForm, setEditForm] = useState<ProfileForm>(emptyForm)
  const [pendingDelete, setPendingDelete] = useState<GatewayProfile | null>(null)

  const load = () => {
    Promise.all([api.profiles(), api.budgets()])
      .then(([p, b]) => {
        setProfiles(p.profiles)
        setBudgets(b.budgets)
      })
      .catch((e) => setError(errorText(e)))
  }

  useEffect(() => {
    load()
  }, [])

  const create = async () => {
    try {
      const allowed_models = form.useDefault ? ['openai:*', 'anthropic:*'] : form.models
      await api.createProfile({
        name: form.name.trim(),
        description: form.description.trim() || undefined,
        allowed_models,
        routes: stepsFromDrafts(form.routes),
        budget_id: form.budget_id || undefined,
        max_reasoning_effort: form.max_reasoning_effort || undefined,
        enabled: form.enabled,
      })
      setShowCreate(false)
      setForm(emptyForm())
      load()
    } catch (e) {
      setError(errorText(e))
    }
  }

  const openEdit = (profile: GatewayProfile) => {
    setEditing(profile)
    setEditForm({
      name: profile.name,
      description: profile.description ?? '',
      budget_id: profile.budget_id ?? '',
      max_reasoning_effort: profile.max_reasoning_effort ?? '',
      enabled: profile.enabled,
      models: [...profile.allowed_models],
      useDefault: false,
      routes: draftsFromRoutes(profile.routes),
    })
  }

  const saveEdit = async () => {
    if (!editing) return
    try {
      await api.updateProfile(editing.id, {
        name: editForm.name.trim(),
        description: editForm.description.trim() || null,
        allowed_models: editForm.models,
        routes: stepsFromDrafts(editForm.routes),
        budget_id: editForm.budget_id || null,
        max_reasoning_effort: editForm.max_reasoning_effort || null,
        enabled: editForm.enabled,
      })
      setEditing(null)
      load()
    } catch (e) {
      setError(errorText(e))
    }
  }

  const toggleEnabled = async (profile: GatewayProfile) => {
    try {
      await api.updateProfile(profile.id, { enabled: !profile.enabled })
      load()
    } catch (e) {
      setError(errorText(e))
    }
  }

  const confirmDelete = async () => {
    if (!pendingDelete) return
    try {
      const result = await api.deleteProfile(pendingDelete.id)
      setPendingDelete(null)
      setNotice(`Deleted profile; cleared ${result.users_cleared} user link(s).`)
      load()
    } catch (e) {
      setError(errorText(e))
      setPendingDelete(null)
    }
  }

  const budgetById = Object.fromEntries(budgets.map((b) => [b.id, b]))

  return (
    <>
      <PageHeader
        title="Profiles"
        description="Named defaults for models, budget, and reasoning. A route forces Chat and Responses for users on that profile."
        actions={<Button onClick={() => setShowCreate(true)}>Create profile</Button>}
      />
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      {notice && <Alert><AlertDescription>{notice}</AlertDescription></Alert>}
      <PaginatedTable
        rows={profiles}
        searchKeys={(p) => [p.id, p.name, p.description ?? '', ...p.allowed_models, ...(p.routes ?? []).map((step) => step.model)]}
        searchPlaceholder="Search profiles…"
      >
        {(pageRows) => (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>ID</TableHead>
                <TableHead>Budget</TableHead>
                <TableHead>Reasoning</TableHead>
                <TableHead>Models</TableHead>
                <TableHead>Route</TableHead>
                <TableHead>Users</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pageRows.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={9} className="text-center text-muted-foreground">
                    No profiles.
                  </TableCell>
                </TableRow>
              ) : pageRows.map((p) => {
                const budget = p.budget_id ? budgetById[p.budget_id] : undefined
                return (
                  <TableRow key={p.id}>
                    <TableCell className="whitespace-normal">
                      <div className="grid gap-0.5">
                        <span className="font-medium">{p.name}</span>
                        {p.description ? (
                          <span className="text-xs text-muted-foreground">{p.description}</span>
                        ) : null}
                      </div>
                    </TableCell>
                    <TableCell className="font-mono" title={p.id}>{shortId(p.id)}</TableCell>
                    <TableCell className="font-mono" title={p.budget_id ?? undefined}>
                      {budget
                        ? `${shortId(budget.id)} ($${budget.max_budget})`
                        : p.budget_id
                          ? shortId(p.budget_id)
                          : '—'}
                    </TableCell>
                    <TableCell className="font-mono">
                      {p.max_reasoning_effort ?? '—'}
                    </TableCell>
                    <TableCell><ChipRow items={p.allowed_models} /></TableCell>
                    <TableCell className="whitespace-normal text-xs">
                      {routeSummary(p.routes) ?? '—'}
                    </TableCell>
                    <TableCell>{p.user_count}</TableCell>
                    <TableCell>
                      <Badge variant={p.enabled ? 'default' : 'secondary'}>
                        {p.enabled ? 'enabled' : 'disabled'}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <RowActions>
                        <Button type="button" variant="outline" size="sm" onClick={() => openEdit(p)}>
                          Edit
                        </Button>
                        <Button type="button" variant="outline" size="sm" onClick={() => toggleEnabled(p)}>
                          {p.enabled ? 'Disable' : 'Enable'}
                        </Button>
                        <Button type="button" variant="destructive" size="sm" onClick={() => setPendingDelete(p)}>
                          Delete
                        </Button>
                      </RowActions>
                    </TableCell>
                  </TableRow>
                )
              })}
            </TableBody>
          </Table>
        )}
      </PaginatedTable>

      <Dialog open={showCreate} onOpenChange={setShowCreate}>
        <DialogContent className="sm:max-w-2xl max-h-[90vh] overflow-y-auto">
          <DialogHeader><DialogTitle>Create profile</DialogTitle></DialogHeader>
          <ProfileFormFields form={form} setForm={setForm} budgets={budgets} />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button
              disabled={!formReady(form)}
              onClick={create}
            >
              Create
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={editing !== null} onOpenChange={(open) => { if (!open) setEditing(null) }}>
        <DialogContent className="sm:max-w-2xl max-h-[90vh] overflow-y-auto">
          <DialogHeader><DialogTitle>Edit profile</DialogTitle></DialogHeader>
          <ProfileFormFields
            form={editForm}
            setForm={setEditForm}
            budgets={budgets}
            hideDefaultOption
          />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setEditing(null)}>Cancel</Button>
            <Button
              disabled={!formReady({ ...editForm, useDefault: false })}
              onClick={saveEdit}
            >
              Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={pendingDelete !== null} onOpenChange={(open) => { if (!open) setPendingDelete(null) }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete profile</DialogTitle>
            <DialogDescription>
              {`Delete profile ${pendingDelete?.name}? User profile links and inherited budgets clear.`}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setPendingDelete(null)}>Cancel</Button>
            <Button variant="destructive" onClick={confirmDelete}>Delete</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

function ProfileFormFields({
  form,
  setForm,
  budgets,
  hideDefaultOption = false,
}: {
  form: ProfileForm
  setForm: (form: ProfileForm) => void
  budgets: GatewayBudget[]
  hideDefaultOption?: boolean
}) {
  return (
    <div className="grid gap-3">
      <div className="grid gap-1.5">
        <Label htmlFor="profile-name">Name</Label>
        <Input
          id="profile-name"
          value={form.name}
          onChange={(e) => setForm({ ...form, name: e.target.value })}
        />
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor="profile-description">Description</Label>
        <Input
          id="profile-description"
          value={form.description}
          onChange={(e) => setForm({ ...form, description: e.target.value })}
        />
      </div>
      <div className="grid gap-1.5">
        <Label>Budget</Label>
        <Select
          value={form.budget_id || 'none'}
          onValueChange={(value) => setForm({ ...form, budget_id: value === 'none' ? '' : value })}
        >
          <SelectTrigger><SelectValue placeholder="—" /></SelectTrigger>
          <SelectContent>
            <SelectItem value="none">—</SelectItem>
            {budgets.map((b) => (
              <SelectItem key={b.id} value={b.id}>
                {shortId(b.id)} (${b.max_budget})
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor="profile-reasoning" className="inline-flex items-center gap-1">
          Max reasoning effort
          <InfoTip label="Reasoning cap">
            Copied onto new keys when they inherit this profile.
          </InfoTip>
        </Label>
        <Select
          value={form.max_reasoning_effort || 'unset'}
          onValueChange={(value) =>
            setForm({ ...form, max_reasoning_effort: value === 'unset' ? '' : value })
          }
        >
          <SelectTrigger id="profile-reasoning"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="unset">No cap</SelectItem>
            {REASONING_LEVELS.map((level) => (
              <SelectItem key={level} value={level}>{level}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <label className="check-row">
        <Checkbox
          checked={form.enabled}
          onCheckedChange={(value) => setForm({ ...form, enabled: value === true })}
        />
        <span>Enabled</span>
      </label>
      <ModelPicker
        selected={form.models}
        useDefault={form.useDefault}
        hideDefaultOption={hideDefaultOption}
        onChange={(models, useDefault) => setForm({ ...form, models, useDefault })}
      />
      <RouteEditor
        idPrefix={hideDefaultOption ? 'edit' : 'create'}
        routes={form.routes}
        onChange={(routes) => setForm({ ...form, routes })}
      />
    </div>
  )
}

function RouteEditor({
  idPrefix,
  routes,
  onChange,
}: {
  idPrefix: string
  routes: RouteDraft[]
  onChange: (routes: RouteDraft[]) => void
}) {
  const catalog = useModelCatalog().ids.filter((id) => id.includes(':') && !id.includes('*'))
  const error = routeError(routes)
  const update = (index: number, patch: Partial<RouteDraft>) => {
    onChange(routes.map((step, stepIndex) => (stepIndex === index ? { ...step, ...patch } : step)))
  }
  const move = (index: number, direction: -1 | 1) => {
    const next = index + direction
    if (next < 0 || next >= routes.length) return
    const copy = [...routes]
    const [item] = copy.splice(index, 1)
    copy.splice(next, 0, item)
    onChange(copy)
  }

  return (
    <div className="grid gap-2">
      <div className="flex items-center gap-1.5">
        <Label>Route</Label>
        <InfoTip label="Profile route">
          Chat and Responses use these steps in order. The model in the request is ignored.
          Embeddings, audio, and System One still use the model list above.
        </InfoTip>
      </div>
      {routes.map((step, index) => (
        <div key={index} className="grid gap-2 rounded-lg border p-2">
          <div className="grid gap-1.5">
            <Label htmlFor={`${idPrefix}-route-model-${index}`}>Step {index + 1} model</Label>
            <Input
              id={`${idPrefix}-route-model-${index}`}
              list={`${idPrefix}-route-model-catalog`}
              value={step.model}
              placeholder="openai:gpt-4o"
              onChange={(event) => update(index, { model: event.target.value })}
            />
          </div>
          <div className="grid gap-2 sm:grid-cols-2">
            <div className="grid gap-1.5">
              <Label htmlFor={`${idPrefix}-route-cap-${index}`}>Spend cap (USD)</Label>
              <Input
                id={`${idPrefix}-route-cap-${index}`}
                type="number"
                min="0"
                step="0.01"
                value={step.untilUsd}
                placeholder={index + 1 === routes.length ? 'Rest of window' : 'Required'}
                onChange={(event) => update(index, { untilUsd: event.target.value })}
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor={`${idPrefix}-route-reasoning-${index}`}>Reasoning</Label>
              <Select
                value={step.reasoning || 'unset'}
                onValueChange={(value) => update(index, { reasoning: value === 'unset' ? '' : value })}
              >
                <SelectTrigger id={`${idPrefix}-route-reasoning-${index}`}><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="unset">Profile cap</SelectItem>
                  {REASONING_LEVELS.map((level) => (
                    <SelectItem key={level} value={level}>{level}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button type="button" variant="outline" size="sm" disabled={index === 0} onClick={() => move(index, -1)}>
              Move up
            </Button>
            <Button type="button" variant="outline" size="sm" disabled={index + 1 === routes.length} onClick={() => move(index, 1)}>
              Move down
            </Button>
            <Button type="button" variant="outline" size="sm" onClick={() => onChange(routes.filter((_, stepIndex) => stepIndex !== index))}>
              Remove
            </Button>
          </div>
        </div>
      ))}
      <datalist id={`${idPrefix}-route-model-catalog`}>
        {catalog.map((id) => <option key={id} value={id} />)}
      </datalist>
      {error ? <p className="m-0 text-xs text-destructive">{error}</p> : null}
      <Button
        type="button"
        variant="outline"
        disabled={routes.length >= MAX_ROUTE_STEPS}
        onClick={() => onChange([...routes, { model: '', untilUsd: '', reasoning: '' }])}
      >
        Add step
      </Button>
    </div>
  )
}
