import { useEffect, useState } from 'react'
import { api } from '../../api'
import { FamilyBadge } from '../../components/FamilyBadge'
import { InfoTip } from '../../components/InfoTip'
import { PageHeader } from '../../components/PageHeader'
import { providerFamily, providerTooltip } from '../../modelFamily'
import type { GatewayProviderStatus } from '../../types'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
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
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { errorText } from '@/lib/utils'

type CreateForm = {
  slug: string
  label: string
  base_url: string
  api_key: string
  enabled: boolean
}

type EditForm = CreateForm & { clear_key: boolean }

const emptyCreate = (): CreateForm => ({
  slug: '',
  label: '',
  base_url: '',
  api_key: '',
  enabled: true,
})

export function GatewayProvidersPage() {
  const [providers, setProviders] = useState<GatewayProviderStatus[]>([])
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [createForm, setCreateForm] = useState<CreateForm>(emptyCreate())
  const [editing, setEditing] = useState<EditForm | null>(null)
  const [pendingDelete, setPendingDelete] = useState<GatewayProviderStatus | null>(null)

  const load = () => {
    api.providers()
      .then((r) => setProviders(r.providers))
      .catch((e) => setError(errorText(e)))
  }

  useEffect(() => {
    load()
  }, [])

  const create = async () => {
    try {
      const body: {
        slug: string
        label?: string
        base_url: string
        api_key?: string
        enabled: boolean
      } = {
        slug: createForm.slug.trim(),
        base_url: createForm.base_url.trim(),
        enabled: createForm.enabled,
      }
      if (createForm.label.trim()) body.label = createForm.label.trim()
      if (createForm.api_key.trim()) body.api_key = createForm.api_key
      await api.createProxyProvider(body)
      setShowCreate(false)
      setCreateForm(emptyCreate())
      setNotice(`Created proxy provider ${body.slug}. Call it as ${body.slug}:model.`)
      load()
    } catch (e) {
      setError(errorText(e))
    }
  }

  const saveEdit = async () => {
    if (!editing) return
    try {
      const body: { label?: string; base_url?: string; api_key?: string; enabled?: boolean } = {
        label: editing.label.trim(),
        base_url: editing.base_url.trim(),
        enabled: editing.enabled,
      }
      if (editing.clear_key) body.api_key = ''
      else if (editing.api_key.trim()) body.api_key = editing.api_key
      await api.updateProxyProvider(editing.slug, body)
      setEditing(null)
      setNotice(`Updated proxy provider ${editing.slug}.`)
      load()
    } catch (e) {
      setError(errorText(e))
    }
  }

  const confirmDelete = async () => {
    if (!pendingDelete) return
    try {
      await api.deleteProxyProvider(pendingDelete.id)
      setNotice(`Deleted proxy provider ${pendingDelete.id}.`)
      setPendingDelete(null)
      load()
    } catch (e) {
      setError(errorText(e))
      setPendingDelete(null)
    }
  }

  return (
    <>
      <PageHeader
        title="Providers"
        description="Builtin credential status and self-hosted proxy providers. A proxy model id is slug:model."
        actions={<Button onClick={() => setShowCreate(true)}>Add proxy</Button>}
      />
      {error && (
        <Alert variant="destructive" className="mb-3">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
      {notice && (
        <Alert className="mb-3">
          <AlertDescription>{notice}</AlertDescription>
        </Alert>
      )}
      <Card>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Provider</TableHead>
                <TableHead>Family</TableHead>
                <TableHead>Configured</TableHead>
                <TableHead>Base URL</TableHead>
                <TableHead>Key</TableHead>
                <TableHead>Catalog</TableHead>
                <TableHead>Models</TableHead>
                <TableHead>Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {providers.map((p) => (
                <TableRow key={`${p.kind}-${p.id}`}>
                  <TableCell className="font-medium">
                    <span className="inline-flex items-center gap-1">
                      {p.label}
                      <InfoTip label={`${p.label} capabilities`}>
                        {p.kind === 'proxy'
                          ? 'Self-hosted OpenAI-compatible upstream. The gateway forwards chat, responses, embeddings, and the model list.'
                          : providerTooltip(p.id)}
                      </InfoTip>
                    </span>
                    {p.kind === 'proxy' && (
                      <div className="font-mono text-xs text-muted-foreground">{p.id}:model</div>
                    )}
                  </TableCell>
                  <TableCell>
                    {p.kind === 'proxy' ? (
                      <Badge variant="secondary">Proxy</Badge>
                    ) : (
                      <FamilyBadge family={providerFamily(p.id)} />
                    )}
                  </TableCell>
                  <TableCell>
                    <Badge variant={p.configured && p.enabled !== false ? 'default' : 'secondary'}>
                      {p.kind === 'proxy' ? (p.enabled ? 'enabled' : 'disabled') : p.configured ? 'yes' : 'no'}
                    </Badge>
                  </TableCell>
                  <TableCell className="font-mono" title={p.base_url}>{p.base_url}</TableCell>
                  <TableCell className="font-mono">{p.key_suffix ?? '—'}</TableCell>
                  <TableCell>
                    {p.configured && p.enabled !== false ? (
                      <Badge variant={p.catalog_ok ? 'default' : 'destructive'}>
                        {p.catalog_ok ? 'ok' : 'error'}
                      </Badge>
                    ) : (
                      '—'
                    )}
                  </TableCell>
                  <TableCell>
                    {p.catalog_ok ? (
                      p.model_count
                    ) : p.error ? (
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <span className="text-destructive cursor-help">error</span>
                        </TooltipTrigger>
                        <TooltipContent>{p.error}</TooltipContent>
                      </Tooltip>
                    ) : (
                      '—'
                    )}
                  </TableCell>
                  <TableCell>
                    {p.kind === 'proxy' && (
                      <div className="flex gap-2">
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() =>
                            setEditing({
                              slug: p.id,
                              label: p.label,
                              base_url: p.base_url,
                              api_key: '',
                              enabled: p.enabled !== false,
                              clear_key: false,
                            })
                          }
                        >
                          Edit
                        </Button>
                        <Button variant="outline" size="sm" onClick={() => setPendingDelete(p)}>
                          Delete
                        </Button>
                      </div>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Dialog open={showCreate} onOpenChange={setShowCreate}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Add proxy provider</DialogTitle>
            <DialogDescription>
              The slug becomes the model prefix. Leave the API key empty when the server does not use one.
              Do not include /v1 on the base URL.
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-3">
            <div className="grid gap-1">
              <Label htmlFor="proxy-slug">Slug</Label>
              <Input
                id="proxy-slug"
                value={createForm.slug}
                onChange={(e) => setCreateForm({ ...createForm, slug: e.target.value })}
                placeholder="homelab"
              />
            </div>
            <div className="grid gap-1">
              <Label htmlFor="proxy-label">Label</Label>
              <Input
                id="proxy-label"
                value={createForm.label}
                onChange={(e) => setCreateForm({ ...createForm, label: e.target.value })}
                placeholder="Homelab"
              />
            </div>
            <div className="grid gap-1">
              <Label htmlFor="proxy-base">Base URL</Label>
              <Input
                id="proxy-base"
                value={createForm.base_url}
                onChange={(e) => setCreateForm({ ...createForm, base_url: e.target.value })}
                placeholder="http://127.0.0.1:11434"
              />
            </div>
            <div className="grid gap-1">
              <Label htmlFor="proxy-key">API key</Label>
              <Input
                id="proxy-key"
                type="password"
                value={createForm.api_key}
                onChange={(e) => setCreateForm({ ...createForm, api_key: e.target.value })}
                autoComplete="off"
              />
            </div>
            <label className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={createForm.enabled}
                onCheckedChange={(checked) => setCreateForm({ ...createForm, enabled: checked === true })}
              />
              Enabled
            </label>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button onClick={create}>Create</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={editing !== null} onOpenChange={(open) => !open && setEditing(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Edit {editing?.slug}</DialogTitle>
            <DialogDescription>
              Leave the API key blank to keep the stored key. Clear it when the server does not use one.
            </DialogDescription>
          </DialogHeader>
          {editing && (
            <div className="grid gap-3">
              <div className="grid gap-1">
                <Label htmlFor="edit-label">Label</Label>
                <Input
                  id="edit-label"
                  value={editing.label}
                  onChange={(e) => setEditing({ ...editing, label: e.target.value })}
                />
              </div>
              <div className="grid gap-1">
                <Label htmlFor="edit-base">Base URL</Label>
                <Input
                  id="edit-base"
                  value={editing.base_url}
                  onChange={(e) => setEditing({ ...editing, base_url: e.target.value })}
                />
              </div>
              <div className="grid gap-1">
                <Label htmlFor="edit-key">API key</Label>
                <Input
                  id="edit-key"
                  type="password"
                  value={editing.api_key}
                  disabled={editing.clear_key}
                  onChange={(e) => setEditing({ ...editing, api_key: e.target.value })}
                  placeholder="Leave blank to keep"
                  autoComplete="off"
                />
              </div>
              <label className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={editing.clear_key}
                  onCheckedChange={(checked) => setEditing({ ...editing, clear_key: checked === true })}
                />
                Clear stored API key
              </label>
              <label className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={editing.enabled}
                  onCheckedChange={(checked) => setEditing({ ...editing, enabled: checked === true })}
                />
                Enabled
              </label>
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditing(null)}>Cancel</Button>
            <Button onClick={saveEdit}>Save</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={pendingDelete !== null} onOpenChange={(open) => !open && setPendingDelete(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete {pendingDelete?.id}?</DialogTitle>
            <DialogDescription>
              Callers can no longer use this prefix. This does not delete usage rows.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setPendingDelete(null)}>Cancel</Button>
            <Button variant="destructive" onClick={confirmDelete}>Delete</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
