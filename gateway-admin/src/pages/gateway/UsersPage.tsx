import { useEffect, useState } from 'react'
import { api } from '../../api'
import { shortId } from '../../components/compactDisplay'
import { InfoTip } from '../../components/InfoTip'
import { PageHeader } from '../../components/PageHeader'
import { PaginatedTable } from '../../components/PaginatedTable'
import type { GatewayBudget, GatewayProfile, GatewayUser } from '../../types'
import { Alert, AlertDescription } from '@/components/ui/alert'
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

function formatReset(iso: string | null) {
  if (!iso) return '—'
  const d = new Date(iso)
  const diff = d.getTime() - Date.now()
  if (diff <= 0) return 'due now'
  const hrs = Math.floor(diff / 3_600_000)
  if (hrs < 48) return `in ${hrs}h`
  return d.toLocaleDateString()
}

function SpendBar({ spend, budget }: { spend: number; budget: GatewayBudget | undefined }) {
  if (!budget) return null
  const pct = Math.min(100, (spend / budget.max_budget) * 100)
  return (
    <div className="progress-wrap" title={`$${spend.toFixed(2)} / $${budget.max_budget.toFixed(2)}`}>
      <div className="progress-track"><div className="progress-fill" style={{ width: `${pct}%` }} /></div>
      <span className="toolbar-meta text-xs text-muted-foreground">{pct.toFixed(0)}%</span>
    </div>
  )
}

export function GatewayUsersPage() {
  const [users, setUsers] = useState<GatewayUser[]>([])
  const [budgets, setBudgets] = useState<GatewayBudget[]>([])
  const [profiles, setProfiles] = useState<GatewayProfile[]>([])
  const [error, setError] = useState<string | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState({ user_id: '', alias: '', profile_id: '' })
  const [editingAlias, setEditingAlias] = useState<{ id: string; alias: string } | null>(null)
  const [pendingDelete, setPendingDelete] = useState<GatewayUser | null>(null)
  const [deleteResult, setDeleteResult] = useState<string | null>(null)

  const budgetMap = Object.fromEntries(budgets.map((b) => [b.id, b]))
  const profileMap = Object.fromEntries(profiles.map((p) => [p.id, p]))
  const selectedProfile = form.profile_id ? profileMap[form.profile_id] : undefined

  const load = () => {
    Promise.all([api.users(), api.budgets(), api.profiles()])
      .then(([u, b, p]) => {
        setUsers(u.users)
        setBudgets(b.budgets)
        setProfiles(p.profiles)
      })
      .catch((e) => setError(String(e)))
  }

  useEffect(() => { load() }, [])

  const create = async () => {
    try {
      await api.createUser({
        user_id: form.user_id,
        alias: form.alias || undefined,
        profile_id: form.profile_id || undefined,
      })
      setShowCreate(false)
      setForm({ user_id: '', alias: '', profile_id: '' })
      load()
    } catch (e) {
      setError(String(e))
    }
  }

  const updateProfile = async (id: string, profile_id: string) => {
    try {
      await api.updateUser(id, { profile_id: profile_id || null })
      load()
    } catch (e) {
      setError(String(e))
    }
  }

  const saveAlias = async () => {
    if (!editingAlias) return
    try {
      await api.updateUser(editingAlias.id, { alias: editingAlias.alias || undefined })
      setEditingAlias(null)
      load()
    } catch (e) {
      setError(String(e))
    }
  }

  const confirmDelete = async () => {
    if (!pendingDelete) return
    try {
      const result = await api.deleteUser(pendingDelete.id)
      setPendingDelete(null)
      setDeleteResult(`Deleted user; ${result.keys_deleted} key(s) removed.`)
      load()
    } catch (e) {
      setError(String(e))
      setPendingDelete(null)
    }
  }

  const enabledProfiles = profiles.filter((p) => p.enabled)

  return (
    <>
      <PageHeader
        title="Users"
        description="Gateway users receive usage attribution. Budget and model defaults come from the assigned profile."
        actions={<Button onClick={() => setShowCreate(true)}>Create user</Button>}
      />
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      {deleteResult && <Alert><AlertDescription>{deleteResult}</AlertDescription></Alert>}
      <PaginatedTable
        rows={users}
        searchKeys={(u) => [u.id, u.alias ?? '', u.profile_id ?? '']}
        searchPlaceholder="Search users…"
      >
        {(pageRows) => (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>ID</TableHead>
                <TableHead>Alias</TableHead>
                <TableHead>Profile</TableHead>
                <TableHead>
                  <span className="inline-flex items-center gap-1">
                    Budget
                    <InfoTip label="Budget">
                      Copied from the assigned profile. Change the profile (or the profile’s budget) to update it.
                    </InfoTip>
                  </span>
                </TableHead>
                <TableHead>
                  <span className="inline-flex items-center gap-1">
                    Spend
                    <InfoTip label="Spend">
                      Current period spend for this user. The bar shows spend against the profile budget max.
                    </InfoTip>
                  </span>
                </TableHead>
                <TableHead>
                  <span className="inline-flex items-center gap-1">
                    Reset
                    <InfoTip label="Budget reset">
                      When the next lazy budget reset is due for this user.
                    </InfoTip>
                  </span>
                </TableHead>
                <TableHead>Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pageRows.map((u) => {
                const profile = u.profile_id ? profileMap[u.profile_id] : undefined
                const budget = u.budget_id ? budgetMap[u.budget_id] : undefined
                return (
                  <TableRow key={u.id}>
                    <TableCell className="font-mono" title={u.id}>{shortId(u.id)}</TableCell>
                    <TableCell>
                      <span className="cell-inline">
                        <span>{u.alias ?? '—'}</span>
                        <Button type="button" variant="outline" size="sm" onClick={() => setEditingAlias({ id: u.id, alias: u.alias ?? '' })}>Edit</Button>
                      </span>
                    </TableCell>
                    <TableCell>
                      <Select
                        value={u.profile_id ?? 'none'}
                        onValueChange={(value) => updateProfile(u.id, value === 'none' ? '' : value)}
                      >
                        <SelectTrigger size="sm"><SelectValue placeholder="—" /></SelectTrigger>
                        <SelectContent>
                          <SelectItem value="none">—</SelectItem>
                          {profiles.map((p) => (
                            <SelectItem key={p.id} value={p.id} disabled={!p.enabled && u.profile_id !== p.id}>
                              {p.name}{p.enabled ? '' : ' (disabled)'}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </TableCell>
                    <TableCell className="font-mono">
                      {budget
                        ? `${shortId(budget.id)} ($${budget.max_budget})`
                        : profile?.budget_id
                          ? shortId(profile.budget_id)
                          : '—'}
                    </TableCell>
                    <TableCell>
                      <span className="spend-cell">
                        <span className="font-mono">${u.spend.toFixed(2)}</span>
                        <SpendBar spend={u.spend} budget={budget} />
                      </span>
                    </TableCell>
                    <TableCell className="font-mono">{formatReset(u.next_budget_reset_at)}</TableCell>
                    <TableCell className="actions">
                      <Button type="button" variant="destructive" size="sm" onClick={() => setPendingDelete(u)}>Delete</Button>
                    </TableCell>
                  </TableRow>
                )
              })}
            </TableBody>
          </Table>
        )}
      </PaginatedTable>

      <Dialog open={showCreate} onOpenChange={setShowCreate}>
        <DialogContent>
          <DialogHeader><DialogTitle>Create gateway user</DialogTitle></DialogHeader>
          <div className="grid gap-3">
            <div className="grid gap-1.5"><Label htmlFor="gateway-user-id">User ID</Label><Input id="gateway-user-id" value={form.user_id} onChange={(e) => setForm({ ...form, user_id: e.target.value })} /></div>
            <div className="grid gap-1.5"><Label htmlFor="gateway-user-alias">Alias</Label><Input id="gateway-user-alias" value={form.alias} onChange={(e) => setForm({ ...form, alias: e.target.value })} /></div>
            <div className="grid gap-1.5">
              <Label>Profile</Label>
              <Select
                value={form.profile_id || 'none'}
                onValueChange={(value) => setForm({ ...form, profile_id: value === 'none' ? '' : value })}
              >
                <SelectTrigger><SelectValue placeholder="—" /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">—</SelectItem>
                  {enabledProfiles.map((p) => (
                    <SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {selectedProfile?.budget_id ? (
                <p className="m-0 text-xs text-muted-foreground">
                  Inherited budget: {shortId(selectedProfile.budget_id)}
                  {budgetMap[selectedProfile.budget_id]
                    ? ` ($${budgetMap[selectedProfile.budget_id].max_budget})`
                    : ''}
                </p>
              ) : selectedProfile ? (
                <p className="m-0 text-xs text-muted-foreground">This profile has no budget.</p>
              ) : null}
            </div>
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button disabled={!form.user_id} onClick={create}>Create</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={editingAlias !== null} onOpenChange={(open) => { if (!open) setEditingAlias(null) }}>
        <DialogContent>
          <DialogHeader><DialogTitle>Edit alias</DialogTitle></DialogHeader>
          {editingAlias && (
            <div className="grid gap-1.5">
              <Label htmlFor="gateway-user-edit-alias">Alias</Label>
              <Input id="gateway-user-edit-alias" value={editingAlias.alias} onChange={(e) => setEditingAlias({ ...editingAlias, alias: e.target.value })} autoFocus />
            </div>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setEditingAlias(null)}>Cancel</Button>
            <Button onClick={saveAlias}>Save</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={pendingDelete !== null} onOpenChange={(open) => { if (!open) setPendingDelete(null) }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete user</DialogTitle>
            <DialogDescription>
              {`Delete gateway user ${pendingDelete?.id}? Linked keys are removed.`}
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
