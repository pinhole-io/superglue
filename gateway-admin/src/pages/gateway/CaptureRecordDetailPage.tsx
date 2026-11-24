import { useEffect, useRef, useState } from 'react'
import { api } from '../../api'
import { LinkButton } from '../../components/AppShell'
import { compactTime, shortId } from '../../components/compactDisplay'
import { FamilyBadge } from '../../components/FamilyBadge'
import { PageHeader } from '../../components/PageHeader'
import type { CaptureRecordDetail } from '../../types'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { errorText, isAbortError } from '@/lib/utils'

const JSON_PREVIEW_CHARS = 64 * 1024
const SSE_PAGE = 50

function textSize(chars: number): string {
  if (chars < 1024) return `${chars} B`
  const kib = chars / 1024
  if (kib < 1024) return `${kib.toFixed(1)} KiB`
  return `${(kib / 1024).toFixed(1)} MiB`
}

function JsonBlock({ title, value }: { title: string; value: unknown }) {
  const [open, setOpen] = useState(false)
  const cache = useRef<{ value: unknown; full: string } | null>(null)
  if (value == null) return null
  let preview: { text: string; truncated: boolean; fullChars: number } | null = null
  if (open) {
    if (cache.current?.value !== value) {
      cache.current = { value, full: JSON.stringify(value, null, 2) }
    }
    const full = cache.current.full
    preview = full.length <= JSON_PREVIEW_CHARS
      ? { text: full, truncated: false, fullChars: full.length }
      : { text: full.slice(0, JSON_PREVIEW_CHARS), truncated: true, fullChars: full.length }
  }
  return (
    <Card>
      <CardHeader className="pb-2">
        <Button type="button" variant="ghost" className="w-fit" onClick={() => setOpen((current) => !current)}>
          {open ? '▼' : '▶'} {title}
        </Button>
      </CardHeader>
      {preview ? (
        <CardContent>
          {preview.truncated ? (
            <p className="muted">Showing the first 64 KiB of {textSize(preview.fullChars)}.</p>
          ) : null}
          <pre className="m-0 max-h-96 overflow-auto rounded-md bg-muted p-3 font-mono text-xs">
            {preview.text}
          </pre>
        </CardContent>
      ) : null}
    </Card>
  )
}

export function GatewayCaptureRecordDetailPage({ requestId }: { requestId?: string } = {}) {
  const [detail, setDetail] = useState<CaptureRecordDetail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [sseShown, setSseShown] = useState(SSE_PAGE)

  useEffect(() => {
    if (!requestId) return
    const controller = new AbortController()
    setError(null)
    setDetail(null)
    setSseShown(SSE_PAGE)
    api
      .captureRecord(requestId, undefined, controller.signal)
      .then((next) => {
        if (!controller.signal.aborted) setDetail(next)
      })
      .catch((cause) => {
        if (controller.signal.aborted || isAbortError(cause)) return
        setError(errorText(cause))
      })
    return () => controller.abort()
  }, [requestId])

  if (!requestId) {
    return <div className="p-4 text-sm text-muted-foreground">Missing request ID.</div>
  }
  if (error) {
    return (
      <>
        <PageHeader
          title="Capture record"
          description="Detail for a captured request."
          actions={<LinkButton route={{ name: 'capture-records' }}>Back to records</LinkButton>}
        />
        <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>
      </>
    )
  }
  if (!detail) {
    return <div className="p-4 text-sm text-muted-foreground">Loading record…</div>
  }

  const { record } = detail
  const model = record.model_resolved ?? record.model_requested
  const sseRows = record.sse.slice(0, sseShown)
  const sseRemaining = record.sse.length - sseRows.length

  return (
    <>
      <PageHeader
        title="Capture record"
        description="Detail for a captured request. Refresh keeps this URL."
        actions={<LinkButton route={{ name: 'capture-records' }}>Back to records</LinkButton>}
      />
      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-5">
        <Summary label="Request ID" value={shortId(record.request_id)} mono />
        <Summary label="User" value={shortId(record.user_id)} />
        <Summary label="Model" value={model} />
        <Summary label="Duration" value={`${record.duration_ms}ms`} />
        <Summary label="Cost" value={record.cost_usd != null ? `$${record.cost_usd.toFixed(4)}` : '—'} />
      </div>
      <div className="flex items-center gap-2">
        <FamilyBadge id={model} />
      </div>
      {record.error && <Alert variant="destructive"><AlertDescription>{record.error}</AlertDescription></Alert>}
      <Card>
        <CardContent>
          <Table>
            <TableBody>
              <TableRow>
                <TableCell className="font-medium">Started</TableCell>
                <TableCell className="font-mono">{compactTime(record.ts_start)}</TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="font-medium">API</TableCell>
                <TableCell>{record.api}</TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="font-medium">Stream</TableCell>
                <TableCell>{record.stream ? 'yes' : 'no'}</TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="font-medium">Key</TableCell>
                <TableCell>{record.key_id ? shortId(record.key_id) : '—'}</TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="font-medium">Source</TableCell>
                <TableCell>
                  {detail.source}
                  {detail.source_path ? ` · ${detail.source_path}` : ''}
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="font-medium">Tokens</TableCell>
                <TableCell>
                  {record.usage
                    ? `${record.usage.prompt_tokens} prompt + ${record.usage.completion_tokens} completion`
                    : '—'}
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </CardContent>
      </Card>
      <JsonBlock title="Request" value={record.request} />
      {!record.stream && <JsonBlock title="Response" value={record.response} />}
      {record.stream && record.sse.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>
              SSE events ({record.sse.length})
              {record.sse_truncated ? ' · truncated' : ''}
            </CardTitle>
          </CardHeader>
          <CardContent>
            <Table>
              <TableBody>
                {sseRows.map((event, i) => (
                  <TableRow key={i}>
                    <TableCell>{i + 1}</TableCell>
                    <TableCell className="whitespace-pre-wrap font-mono text-xs">{event}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            {sseRemaining > 0 ? (
              <Button
                type="button"
                variant="outline"
                className="mt-3"
                onClick={() => setSseShown((shown) => shown + SSE_PAGE)}
              >
                Show {Math.min(SSE_PAGE, sseRemaining)} more
              </Button>
            ) : null}
          </CardContent>
        </Card>
      )}
    </>
  )
}

function Summary({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <Card size="sm">
      <CardContent>
        <div className="text-xs text-muted-foreground">{label}</div>
        <div className={mono ? 'font-mono font-semibold' : 'font-semibold'}>{value}</div>
      </CardContent>
    </Card>
  )
}
