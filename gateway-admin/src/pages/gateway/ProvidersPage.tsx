import { useEffect, useState } from 'react'
import { api } from '../../api'
import { FamilyBadge } from '../../components/FamilyBadge'
import { InfoTip } from '../../components/InfoTip'
import { PageHeader } from '../../components/PageHeader'
import { providerFamily, providerTooltip } from '../../modelFamily'
import type { GatewayProviderStatus } from '../../types'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

export function GatewayProvidersPage() {
  const [providers, setProviders] = useState<GatewayProviderStatus[]>([])
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    api.providers()
      .then((r) => setProviders(r.providers))
      .catch((e) => setError(String(e)))
  }, [])

  return (
    <>
      <PageHeader
        title="Providers"
        description="Upstream credential and catalog status. System 1 is TypeSafe; other providers are System 2."
      />
      <Card>
        <CardContent>
          {error && (
            <Alert variant="destructive" className="mb-3">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
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
              </TableRow>
            </TableHeader>
            <TableBody>
              {providers.map((p) => (
                <TableRow key={p.id}>
                  <TableCell className="font-medium">
                    <span className="inline-flex items-center gap-1">
                      {p.label}
                      <InfoTip label={`${p.label} capabilities`}>{providerTooltip(p.id)}</InfoTip>
                    </span>
                  </TableCell>
                  <TableCell>
                    <FamilyBadge family={providerFamily(p.id)} />
                  </TableCell>
                  <TableCell>
                    <Badge variant={p.configured ? 'default' : 'secondary'}>
                      {p.configured ? 'yes' : 'no'}
                    </Badge>
                  </TableCell>
                  <TableCell className="font-mono" title={p.base_url}>{p.base_url}</TableCell>
                  <TableCell className="font-mono">{p.key_suffix ?? '—'}</TableCell>
                  <TableCell>
                    {p.configured ? (
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
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </>
  )
}
