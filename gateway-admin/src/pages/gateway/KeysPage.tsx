import { useEffect, useMemo, useState } from 'react'
import { api } from '../../api'
import { shortId, userLabel } from '../../components/compactDisplay'
import { ChipRow } from '../../components/compactUi'
import { FamilyBadge } from '../../components/FamilyBadge'
import { UserSelect } from '../../components/GatewayEntitySelect'
import { InfoTip } from '../../components/InfoTip'
import { ModelPicker } from '../../components/ModelPicker'
import { PageHeader } from '../../components/PageHeader'
import { PaginatedTable } from '../../components/PaginatedTable'
import { classifyModel } from '../../modelFamily'
import type { CreateKeyResponse, GatewayKey, GatewayProfile, GatewayUser } from '../../types'
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
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

const REASONING_LEVELS = ['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'] as const

function parseMetadata(raw: string | null): Record<string, unknown> {
  if (!raw) return {}
  try {
    return JSON.parse(raw) as Record<string, unknown>
  } catch {
    return {}
  }
}

function expiresBadge(iso: string | null) {
  if (!iso) return null
  const d = new Date(iso)
  const soon = d.getTime() - Date.now() < 7 * 86400_000
  return (
    <span className={`status-badge ${soon ? 'status-disabled' : 'status-live'}`}>
      {d.toLocaleDateString()}
    </span>
  )
}

function FamilyChips({ models }: { models: string[] }) {
  const families = [...new Set(models.map(classifyModel))]
  return (
    <div className="flex flex-wrap gap-1">
      {families.map((family) => (
        <FamilyBadge key={family} family={family} />
      ))}
    </div>
  )
}

export function GatewayKeysPage() {
  const [keys, setKeys] = useState<GatewayKey[]>([])
  const [users, setUsers] = useState<GatewayUser[]>([])
  const [profiles, setProfiles] = useState<GatewayProfile[]>([])
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [editing, setEditing] = useState<GatewayKey | null>(null)
  const [plaintextKey, setPlaintextKey] = useState<CreateKeyResponse | null>(null)
  const [pendingRevoke, setPendingRevoke] = useState<GatewayKey | null>(null)
  const [form, setForm] = useState({
    user_id: '',
    name: '',
    models: [] as string[],
    inheritProfile: true,
    expires_at: '',
    max_reasoning_effort: '',
  })
  const [editForm, setEditForm] = useState({
    models: [] as string[],
    expires_at: '',
    max_reasoning_effort: '',
  })

  const userById = Object.fromEntries(users.map((user) => [user.id, user]))
  const profileById = Object.fromEntries(profiles.map((profile) => [profile.id, profile]))

  const selectedUserProfile = useMemo(() => {
    const user = userById[form.user_id]
    if (!user?.profile_id) return undefined
    const profile = profileById[user.profile_id]
    if (!profile?.enabled) return undefined
    return profile
  }, [form.user_id, userById, profileById])

  const load = () => {
    Promise.all([api.keys(), api.users(), api.profiles()])
      .then(([k, u, p]) => {
        setKeys(k.keys)
        setUsers(u.users)
        setProfiles(p.profiles)
      })
      .catch((e) => setError(String(e)))
  }
  useEffect(() => {
    load()
  }, [])

  const create = async () => {
    try {
      const inherit = form.inheritProfile && Boolean(selectedUserProfile)
      const metadata =
        !inherit && form.max_reasoning_effort
          ? { max_reasoning_effort: form.max_reasoning_effort }
          : undefined
      const result = await api.createKey({
        user_id: form.user_id,
        name: form.name || undefined,
        ...(inherit ? {} : { allowed_models: form.models }),
        expires_at: form.expires_at || undefined,
        ...(metadata ? { metadata } : {}),
      })
      setPlaintextKey(result)
      setShowCreate(false)
      setForm({
        user_id: '',
        name: '',
        models: [],
        inheritProfile: true,
        expires_at: '',
        max_reasoning_effort: '',
      })
      load()
    } catch (e) {
      setError(String(e))
    }
  }

  const openEdit = (key: GatewayKey) => {
    const meta = parseMetadata(key.metadata_json)
    setEditing(key)
    setEditForm({
      models: [...key.allowed_models],
      expires_at: key.expires_at?.slice(0, 16) ?? '',
      max_reasoning_effort:
        typeof meta.max_reasoning_effort === 'string' ? meta.max_reasoning_effort : '',
    })
  }

  const saveEdit = async () => {
    if (!editing) return
    try {
      const existing = parseMetadata(editing.metadata_json)
      const metadata = { ...existing }
      if (editForm.max_reasoning_effort) {
        metadata.max_reasoning_effort = editForm.max_reasoning_effort
      } else {
        delete metadata.max_reasoning_effort
      }
      await api.updateKey(editing.id, {
        allowed_models: editForm.models,
        expires_at: editForm.expires_at ? new Date(editForm.expires_at).toISOString() : null,
        metadata,
      })
      setEditing(null)
      load()
    } catch (e) {
      setError(String(e))
    }
  }

  const applyProfileDefaults = async (key: GatewayKey) => {
    const user = userById[key.user_id]
    if (!user?.profile_id) {
      setError('User has no profile assigned.')
      return
    }
    const profile = profileById[user.profile_id]
    if (!profile?.enabled) {
      setError('User profile is missing or disabled.')
      return
    }
    try {
      const existing = parseMetadata(key.metadata_json)
      const metadata = { ...existing }
      if (profile.max_reasoning_effort) {
        metadata.max_reasoning_effort = profile.max_reasoning_effort
      } else {
        delete metadata.max_reasoning_effort
      }
      await api.updateKey(key.id, {
        allowed_models: profile.allowed_models,
        metadata,
      })
      setNotice(`Applied profile “${profile.name}” to key ${shortId(key.key_prefix)}.`)
      load()
    } catch (e) {
      setError(String(e))
    }
  }

  const toggleActive = async (key: GatewayKey) => {
    try {
      await api.updateKey(key.id, { active: !key.active })
      load()
    } catch (e) {
      setError(String(e))
    }
  }

  const confirmRevoke = async () => {
    if (!pendingRevoke) return
    try {
      await api.deleteKey(pendingRevoke.id)
      setPendingRevoke(null)
      load()
    } catch (e) {
      setError(String(e))
      setPendingRevoke(null)
    }
  }

  const canCreate =
    Boolean(form.user_id) &&
    (Boolean(form.inheritProfile && selectedUserProfile) || form.models.length > 0)

  return (
    <>
      <PageHeader
        title="Keys"
        description="Virtual API keys with model allowlists. New keys can inherit models from the user’s profile."
        actions={<Button onClick={() => setShowCreate(true)}>Create key</Button>}
      />
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      {notice && <Alert><AlertDescription>{notice}</AlertDescription></Alert>}
      <PaginatedTable
        rows={keys}
        searchKeys={(k) => [k.key_prefix, k.name ?? '', k.user_id, userLabel(userById[k.user_id], k.user_id), ...k.allowed_models]}
        searchPlaceholder="Search keys…"
      >
        {(pageRows) => (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Prefix</TableHead>
                <TableHead>Name</TableHead>
                <TableHead>User</TableHead>
                <TableHead>Active</TableHead>
                <TableHead>Expires</TableHead>
                <TableHead>Families</TableHead>
                <TableHead>Reasoning cap</TableHead>
                <TableHead>Models</TableHead>
                <TableHead>Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pageRows.map((k) => {
                const meta = parseMetadata(k.metadata_json)
                const user = userById[k.user_id]
                const hasProfile = Boolean(user?.profile_id && profileById[user.profile_id]?.enabled)
                return (
                  <TableRow key={k.id}>
                    <TableCell className="font-mono" title={k.key_prefix}>{shortId(k.key_prefix)}</TableCell>
                    <TableCell>{k.name ?? '—'}</TableCell>
                    <TableCell title={k.user_id}>{userLabel(userById[k.user_id], k.user_id)}</TableCell>
                    <TableCell>
                      <Badge variant={k.active ? 'default' : 'secondary'}>{k.active ? 'yes' : 'no'}</Badge>
                    </TableCell>
                    <TableCell>{expiresBadge(k.expires_at) ?? '—'}</TableCell>
                    <TableCell><FamilyChips models={k.allowed_models} /></TableCell>
                    <TableCell className="font-mono">
                      {typeof meta.max_reasoning_effort === 'string' ? meta.max_reasoning_effort : '—'}
                    </TableCell>
                    <TableCell><ChipRow items={k.allowed_models} /></TableCell>
                    <TableCell className="actions">
                      <Button type="button" variant="outline" size="sm" onClick={() => openEdit(k)}>Edit</Button>
                      {hasProfile ? (
                        <Button type="button" variant="outline" size="sm" onClick={() => applyProfileDefaults(k)}>
                          Apply profile defaults
                        </Button>
                      ) : null}
                      <Button type="button" variant="outline" size="sm" onClick={() => toggleActive(k)}>
                        {k.active ? 'Disable' : 'Enable'}
                      </Button>
                      <Button type="button" variant="destructive" size="sm" onClick={() => setPendingRevoke(k)}>
                        Revoke
                      </Button>
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
          <DialogHeader><DialogTitle>Create gateway key</DialogTitle></DialogHeader>
          <div className="grid gap-3">
            <UserSelect
              id="gateway-key-user"
              users={users}
              value={form.user_id}
              onChange={(user_id) => setForm({ ...form, user_id, inheritProfile: true })}
              className="grid gap-1.5"
            />
            <div className="grid gap-1.5">
              <Label htmlFor="gateway-key-name">Name</Label>
              <Input id="gateway-key-name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="gateway-key-expires">Expires</Label>
              <Input
                id="gateway-key-expires"
                type="datetime-local"
                value={form.expires_at}
                onChange={(e) => setForm({ ...form, expires_at: e.target.value })}
              />
            </div>
            {selectedUserProfile && form.inheritProfile ? (
              <Alert>
                <AlertDescription>
                  From profile “{selectedUserProfile.name}”: {selectedUserProfile.allowed_models.join(', ')}
                  {selectedUserProfile.max_reasoning_effort
                    ? ` · reasoning ${selectedUserProfile.max_reasoning_effort}`
                    : ''}
                  {' '}
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="ml-2"
                    onClick={() =>
                      setForm({
                        ...form,
                        inheritProfile: false,
                        models: [...selectedUserProfile.allowed_models],
                        max_reasoning_effort: selectedUserProfile.max_reasoning_effort ?? '',
                      })
                    }
                  >
                    Customize
                  </Button>
                </AlertDescription>
              </Alert>
            ) : (
              <>
                {!selectedUserProfile && form.user_id ? (
                  <p className="m-0 text-xs text-muted-foreground">
                    This user has no enabled profile. Choose models below.
                  </p>
                ) : null}
                <div className="grid gap-1.5">
                  <Label htmlFor="gateway-key-reasoning" className="inline-flex items-center gap-1">
                    Max reasoning effort
                    <InfoTip label="Reasoning cap">
                      Caps reasoning_effort on System 2 chat calls for this key.
                    </InfoTip>
                  </Label>
                  <Select
                    value={form.max_reasoning_effort || 'unset'}
                    onValueChange={(value) =>
                      setForm({ ...form, max_reasoning_effort: value === 'unset' ? '' : value })
                    }
                  >
                    <SelectTrigger id="gateway-key-reasoning"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="unset">No cap</SelectItem>
                      {REASONING_LEVELS.map((level) => (
                        <SelectItem key={level} value={level}>{level}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <ModelPicker
                  selected={form.models}
                  useDefault={false}
                  hideDefaultOption
                  onChange={(models) => setForm({ ...form, models, inheritProfile: false })}
                />
              </>
            )}
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button disabled={!canCreate} onClick={create}>
              Create
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={editing !== null} onOpenChange={(open) => { if (!open) setEditing(null) }}>
        <DialogContent className="sm:max-w-2xl max-h-[90vh] overflow-y-auto">
          <DialogHeader><DialogTitle>Edit key {editing?.key_prefix}</DialogTitle></DialogHeader>
          <div className="grid gap-3">
            <div className="grid gap-1.5">
              <Label htmlFor="gateway-key-edit-expires">Expires (clear to remove)</Label>
              <Input
                id="gateway-key-edit-expires"
                type="datetime-local"
                value={editForm.expires_at}
                onChange={(e) => setEditForm({ ...editForm, expires_at: e.target.value })}
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="gateway-key-edit-reasoning" className="inline-flex items-center gap-1">
                Max reasoning effort
                <InfoTip label="Reasoning cap">
                  Caps reasoning_effort on System 2 chat calls for this key.
                </InfoTip>
              </Label>
              <Select
                value={editForm.max_reasoning_effort || 'unset'}
                onValueChange={(value) =>
                  setEditForm({ ...editForm, max_reasoning_effort: value === 'unset' ? '' : value })
                }
              >
                <SelectTrigger id="gateway-key-edit-reasoning"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="unset">No cap</SelectItem>
                  {REASONING_LEVELS.map((level) => (
                    <SelectItem key={level} value={level}>{level}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <ModelPicker
              selected={editForm.models}
              useDefault={false}
              hideDefaultOption
              onChange={(models) => setEditForm({ ...editForm, models })}
            />
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setEditing(null)}>Cancel</Button>
            <Button disabled={editForm.models.length === 0} onClick={saveEdit}>Save</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={plaintextKey !== null} onOpenChange={(open) => { if (!open) setPlaintextKey(null) }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="inline-flex items-center gap-1">
              Key created — copy now
              <InfoTip label="One-time secret">
                The plaintext key is shown once. Store it now. The gateway keeps only a hash.
              </InfoTip>
            </DialogTitle>
            <DialogDescription>This secret is shown once.</DialogDescription>
          </DialogHeader>
          {plaintextKey && (
            <pre className="max-h-48 overflow-auto rounded-md bg-muted p-3 font-mono text-xs">
              {plaintextKey.key}
            </pre>
          )}
          <DialogFooter>
            <Button
              onClick={() => {
                if (plaintextKey) void navigator.clipboard.writeText(plaintextKey.key)
                setPlaintextKey(null)
              }}
            >
              Copy & close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={pendingRevoke !== null} onOpenChange={(open) => { if (!open) setPendingRevoke(null) }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Revoke key</DialogTitle>
            <DialogDescription>
              Revoke key {pendingRevoke?.key_prefix}? Clients that use it will fail auth.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setPendingRevoke(null)}>Cancel</Button>
            <Button variant="destructive" onClick={confirmRevoke}>Revoke</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
