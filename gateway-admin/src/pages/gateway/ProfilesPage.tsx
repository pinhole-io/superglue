import { useEffect, useState } from 'react'
import { api } from '../../api'
import { shortId } from '../../components/compactDisplay'
import { ChipRow } from '../../components/compactUi'
import { InfoTip } from '../../components/InfoTip'
import { ModelPicker } from '../../components/ModelPicker'
import { PageHeader } from '../../components/PageHeader'
import { PaginatedTable } from '../../components/PaginatedTable'
import type { GatewayBudget, GatewayProfile } from '../../types'
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

const REASONING_LEVELS = ['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'] as const

type ProfileForm = {
  name: string
  description: string
  budget_id: string
  max_reasoning_effort: string
  enabled: boolean
  models: string[]
  useDefault: boolean
}

const emptyForm = (): ProfileForm => ({
  name: '',
  description: '',
  budget_id: '',
  max_reasoning_effort: '',
  enabled: true,
  models: [],
  useDefault: true,
})

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
      .catch((e) => setError(String(e)))
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
        budget_id: form.budget_id || undefined,
        max_reasoning_effort: form.max_reasoning_effort || undefined,
        enabled: form.enabled,
      })
      setShowCreate(false)
      setForm(emptyForm())
      load()
    } catch (e) {
      setError(String(e))
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
    })
  }

  const saveEdit = async () => {
    if (!editing) return
    try {
      await api.updateProfile(editing.id, {
        name: editForm.name.trim(),
        description: editForm.description.trim() || null,
        allowed_models: editForm.models,
        budget_id: editForm.budget_id || null,
        max_reasoning_effort: editForm.max_reasoning_effort || null,
        enabled: editForm.enabled,
      })
      setEditing(null)
      load()
    } catch (e) {
      setError(String(e))
    }
  }

  const toggleEnabled = async (profile: GatewayProfile) => {
    try {
      await api.updateProfile(profile.id, { enabled: !profile.enabled })
      load()
    } catch (e) {
      setError(String(e))
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
      setError(String(e))
      setPendingDelete(null)
    }
  }

  return (
    <>
      <PageHeader
        title="Profiles"
        description="Named defaults for models, budget, and reasoning. Assign a profile to a user; new keys inherit those models."
        actions={<Button onClick={() => setShowCreate(true)}>Create profile</Button>}
      />
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      {notice && <Alert><AlertDescription>{notice}</AlertDescription></Alert>}
      <PaginatedTable
        rows={profiles}
        searchKeys={(p) => [p.id, p.name, p.description ?? '', ...p.allowed_models]}
        searchPlaceholder="Search profiles…"
      >
        {(pageRows) => (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Budget</TableHead>
                <TableHead>Reasoning</TableHead>
                <TableHead>Models</TableHead>
                <TableHead>Users</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pageRows.map((p) => (
                <TableRow key={p.id}>
                  <TableCell>
                    <div className="grid gap-0.5">
                      <span className="font-medium">{p.name}</span>
                      <span className="font-mono text-xs text-muted-foreground" title={p.id}>
                        {shortId(p.id)}
                      </span>
                      {p.description ? (
                        <span className="text-xs text-muted-foreground">{p.description}</span>
                      ) : null}
                    </div>
                  </TableCell>
                  <TableCell className="font-mono">
                    {p.budget_id ? shortId(p.budget_id) : '—'}
                  </TableCell>
                  <TableCell className="font-mono">
                    {p.max_reasoning_effort ?? '—'}
                  </TableCell>
                  <TableCell><ChipRow items={p.allowed_models} /></TableCell>
                  <TableCell>{p.user_count}</TableCell>
                  <TableCell>
                    <Badge variant={p.enabled ? 'default' : 'secondary'}>
                      {p.enabled ? 'enabled' : 'disabled'}
                    </Badge>
                  </TableCell>
                  <TableCell className="actions">
                    <Button type="button" variant="outline" size="sm" onClick={() => openEdit(p)}>
                      Edit
                    </Button>
                    <Button type="button" variant="outline" size="sm" onClick={() => toggleEnabled(p)}>
                      {p.enabled ? 'Disable' : 'Enable'}
                    </Button>
                    <Button type="button" variant="destructive" size="sm" onClick={() => setPendingDelete(p)}>
                      Delete
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
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
              disabled={!form.name.trim() || (!form.useDefault && form.models.length === 0)}
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
              disabled={!editForm.name.trim() || editForm.models.length === 0}
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
    </div>
  )
}
