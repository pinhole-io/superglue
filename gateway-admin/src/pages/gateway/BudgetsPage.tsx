import { useEffect, useState } from 'react'
import { api } from '../../api'
import { compactTime, shortId, userLabel } from '../../components/compactDisplay'
import { InfoTip } from '../../components/InfoTip'
import { PageHeader } from '../../components/PageHeader'
import { PaginatedTable } from '../../components/PaginatedTable'
import type { BudgetResetLog, GatewayBudget, GatewayUser } from '../../types'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
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
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

export function GatewayBudgetsPage() {
  const [budgets, setBudgets] = useState<GatewayBudget[]>([])
  const [resets, setResets] = useState<BudgetResetLog[]>([])
  const [users, setUsers] = useState<GatewayUser[]>([])
  const [error, setError] = useState<string | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState({ max_budget: 10000, duration_sec: 2592000, enforce: true })
  const [editing, setEditing] = useState<GatewayBudget | null>(null)
  const [pendingDelete, setPendingDelete] = useState<GatewayBudget | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  const load = () => {
    Promise.all([
      api.budgets(),
      api.budgetResets({ limit: 50 }),
      api.users().catch(() => ({ users: [] })),
    ])
      .then(([b, r, u]) => {
        setBudgets(b.budgets)
        setResets(r.resets)
        setUsers(u.users)
      })
      .catch((e) => setError(String(e)))
  }
  useEffect(() => { load() }, [])

  const create = async () => {
    try {
      await api.createBudget(form)
      setShowCreate(false)
      load()
    } catch (e) {
      setError(String(e))
    }
  }

  const saveEdit = async () => {
    if (!editing) return
    try {
      await api.updateBudget(editing.id, {
        max_budget: editing.max_budget,
        duration_sec: editing.duration_sec,
        enforce: editing.enforce,
      })
      setEditing(null)
      load()
    } catch (e) {
      setError(String(e))
    }
  }

  const confirmDelete = async () => {
    if (!pendingDelete) return
    try {
      const result = await api.deleteBudget(pendingDelete.id)
      setPendingDelete(null)
      setNotice(`Deleted budget; cleared ${result.users_cleared} user(s).`)
      load()
    } catch (e) {
      setError(String(e))
      setPendingDelete(null)
    }
  }

  return (
    <>
      <PageHeader
        title="Budgets"
        description="Spend limits for gateway users. Enforce rejects at the limit; track-only records spend."
        actions={<Button onClick={() => setShowCreate(true)}>Create budget</Button>}
      />
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      {notice && <Alert><AlertDescription>{notice}</AlertDescription></Alert>}
      <PaginatedTable rows={budgets} searchKeys={(b) => [b.id]} searchPlaceholder="Search budgets…">
        {(pageRows) => (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>ID</TableHead>
                <TableHead>Max</TableHead>
                <TableHead>Duration</TableHead>
                <TableHead>
                  <span className="inline-flex items-center gap-1">
                    Enforce
                    <InfoTip label="Enforce">
                      Enforced budgets reject requests at the limit. Track-only budgets record spend and still allow the call.
                    </InfoTip>
                  </span>
                </TableHead>
                <TableHead>Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pageRows.map((b) => (
                <TableRow key={b.id}>
                  <TableCell className="font-mono" title={b.id}>{shortId(b.id)}</TableCell>
                  <TableCell>${b.max_budget.toFixed(2)}</TableCell>
                  <TableCell>{Math.round(b.duration_sec / 86400)}d</TableCell>
                  <TableCell>{b.enforce ? 'yes' : 'no'}</TableCell>
                  <TableCell className="actions">
                    <Button type="button" variant="outline" size="sm" onClick={() => setEditing({ ...b })}>Edit</Button>
                    <Button type="button" variant="destructive" size="sm" onClick={() => setPendingDelete(b)}>Delete</Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </PaginatedTable>
      <Card>
        <CardHeader><CardTitle>Reset history</CardTitle></CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Reset at</TableHead>
                <TableHead>User</TableHead>
                <TableHead>Budget</TableHead>
                <TableHead>Previous spend</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {resets.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={4} className="text-center text-muted-foreground">
                    No resets recorded yet.
                  </TableCell>
                </TableRow>
              ) : (
                resets.map((r) => (
                  <TableRow key={r.id}>
                    <TableCell className="font-mono" title={r.reset_at}>{compactTime(r.reset_at)}</TableCell>
                    <TableCell title={r.user_id}>
                      {userLabel(users.find((user) => user.id === r.user_id), r.user_id)}
                    </TableCell>
                    <TableCell className="font-mono" title={r.budget_id}>{shortId(r.budget_id)}</TableCell>
                    <TableCell>${r.previous_spend.toFixed(4)}</TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Dialog open={showCreate} onOpenChange={setShowCreate}>
        <DialogContent>
          <DialogHeader><DialogTitle>Create budget</DialogTitle></DialogHeader>
          <BudgetForm form={form} onChange={setForm} />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button onClick={create}>Create</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={editing !== null} onOpenChange={(open) => { if (!open) setEditing(null) }}>
        <DialogContent>
          <DialogHeader><DialogTitle>Edit budget</DialogTitle></DialogHeader>
          {editing ? (
            <BudgetForm
              form={editing}
              onChange={(next) => setEditing((current) => (current ? { ...current, ...next } : current))}
            />
          ) : null}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setEditing(null)}>Cancel</Button>
            <Button onClick={saveEdit}>Save</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={pendingDelete !== null} onOpenChange={(open) => { if (!open) setPendingDelete(null) }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete budget</DialogTitle>
            <DialogDescription>
              Delete this budget? User assignments will be cleared.
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

function BudgetForm({
  form,
  onChange,
}: {
  form: { max_budget: number; duration_sec: number; enforce: boolean }
  onChange: (form: { max_budget: number; duration_sec: number; enforce: boolean }) => void
}) {
  return (
    <div className="grid gap-3">
      <div className="grid gap-1.5">
        <Label htmlFor="gateway-budget-max">Max budget (USD)</Label>
        <Input
          id="gateway-budget-max"
          type="number"
          value={form.max_budget}
          onChange={(e) => onChange({ ...form, max_budget: Number(e.target.value) })}
        />
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor="gateway-budget-duration">Duration (sec)</Label>
        <Input
          id="gateway-budget-duration"
          type="number"
          value={form.duration_sec}
          onChange={(e) => onChange({ ...form, duration_sec: Number(e.target.value) })}
        />
      </div>
      <label className="flex items-center gap-2 text-sm">
        <Checkbox
          checked={form.enforce}
          onCheckedChange={(checked) => onChange({ ...form, enforce: checked === true })}
        />
        <span className="inline-flex items-center gap-1">
          Enforce
          <InfoTip label="Enforce">
            Enforced budgets reject requests at the limit. Track-only budgets record spend and still allow the call.
          </InfoTip>
        </span>
      </label>
    </div>
  )
}
