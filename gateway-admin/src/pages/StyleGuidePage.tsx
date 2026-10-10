import { useState, type ReactNode } from 'react'
import { ChipRow } from '../components/compactUi'
import { FamilyBadge } from '../components/FamilyBadge'
import { InfoTip } from '../components/InfoTip'
import { PageHeader } from '../components/PageHeader'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'
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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

function Section({
  id,
  title,
  description,
  children,
}: {
  id: string
  title: string
  description: string
  children: ReactNode
}) {
  return (
    <section id={id} className="styleguide-section">
      <div className="styleguide-section-heading">
        <h3>{title}</h3>
        <p>{description}</p>
      </div>
      {children}
    </section>
  )
}

function Swatch({ name, value, className }: { name: string; value: string; className: string }) {
  return (
    <div className="styleguide-swatch">
      <div className={`styleguide-swatch-chip ${className}`} />
      <div>
        <div className="font-medium">{name}</div>
        <div className="mono muted">{value}</div>
      </div>
    </div>
  )
}

export function StyleGuidePage() {
  const [checked, setChecked] = useState(true)
  const [selectValue, setSelectValue] = useState('openai')
  const [dialogOpen, setDialogOpen] = useState(false)
  const [text, setText] = useState('sgw-example')

  return (
    <>
      <PageHeader
        title="Style guide"
        description="Shared UI building blocks for the gateway admin. Prefer these components over ad-hoc markup."
      />

      <nav className="styleguide-toc" aria-label="Style guide sections">
        {[
          ['colors', 'Colors'],
          ['type', 'Typography'],
          ['buttons', 'Buttons'],
          ['badges', 'Badges'],
          ['forms', 'Forms'],
          ['alerts', 'Alerts'],
          ['cards', 'Cards'],
          ['table', 'Table'],
          ['tooltip', 'Tooltip'],
          ['dialog', 'Dialog'],
          ['domain', 'Domain'],
        ].map(([id, label]) => (
          <a key={id} href={`#${id}`}>{label}</a>
        ))}
      </nav>

      <Section id="colors" title="Colors" description="Theme tokens from style.css.">
        <div className="styleguide-swatch-grid">
          <Swatch name="Background" value="#0c0c0f" className="bg-background" />
          <Swatch name="Foreground" value="#f4f4f5" className="bg-foreground" />
          <Swatch name="Card" value="#16161a" className="bg-card" />
          <Swatch name="Primary" value="#a78bfa" className="bg-primary" />
          <Swatch name="Secondary" value="#1c1c22" className="bg-secondary" />
          <Swatch name="Muted" value="#1c1c22" className="bg-muted" />
          <Swatch name="Border" value="#2e2e36" className="bg-border" />
          <Swatch name="Destructive" value="#f87171" className="bg-destructive" />
        </div>
      </Section>

      <Section id="type" title="Typography" description="Headings, body copy, and utility text classes.">
        <Card>
          <CardContent className="grid gap-3">
            <p className="eyebrow">SUPERGLUE</p>
            <h1 style={{ margin: 0, fontSize: 24 }}>Heading 1</h1>
            <h2 style={{ margin: 0, fontSize: 18 }}>Heading 2</h2>
            <h3 style={{ margin: 0, fontSize: 14 }}>Heading 3</h3>
            <p style={{ margin: 0 }}>Body text for page copy and form help.</p>
            <p className="muted" style={{ margin: 0 }}>Muted secondary text.</p>
            <p className="mono" style={{ margin: 0 }}>openai:gpt-4o-mini</p>
            <p className="error" style={{ margin: 0 }}>Error message text.</p>
            <p style={{ margin: 0 }}>
              <button type="button" className="link-button">Link button</button>
            </p>
          </CardContent>
        </Card>
      </Section>

      <Section id="buttons" title="Buttons" description="Primary actions, secondary actions, and destructive actions.">
        <Card>
          <CardContent className="flex flex-wrap items-center gap-2">
            <Button>Default</Button>
            <Button variant="secondary">Secondary</Button>
            <Button variant="outline">Outline</Button>
            <Button variant="ghost">Ghost</Button>
            <Button variant="destructive">Destructive</Button>
            <Button variant="link">Link</Button>
            <Button disabled>Disabled</Button>
            <Button size="sm">Small</Button>
            <Button size="lg">Large</Button>
            <Button size="xs">XS</Button>
          </CardContent>
        </Card>
      </Section>

      <Section id="badges" title="Badges" description="Status chips and family labels.">
        <Card>
          <CardContent className="flex flex-wrap items-center gap-2">
            <Badge>Default</Badge>
            <Badge variant="secondary">Secondary</Badge>
            <Badge variant="outline">Outline</Badge>
            <Badge variant="destructive">Destructive</Badge>
            <Badge variant="ghost">Ghost</Badge>
            <FamilyBadge family="system1" />
            <FamilyBadge family="system2" />
            <FamilyBadge family="embeddings" />
            <FamilyBadge family="other" />
            <ChipRow items={['openai:*', 'anthropic:*', 'typesafe:jev-latest']} />
          </CardContent>
        </Card>
      </Section>

      <Section id="forms" title="Forms" description="Labels, inputs, selects, and checkboxes.">
        <Card>
          <CardContent className="grid max-w-md gap-3">
            <div className="grid gap-1.5">
              <Label htmlFor="sg-text">Text input</Label>
              <Input id="sg-text" value={text} onChange={(e) => setText(e.target.value)} placeholder="Value" />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="sg-password">Password</Label>
              <Input id="sg-password" type="password" defaultValue="secret" />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="sg-number">Number</Label>
              <Input id="sg-number" type="number" defaultValue={100} />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="sg-datetime">Datetime</Label>
              <Input id="sg-datetime" type="datetime-local" />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="sg-search">Search</Label>
              <Input id="sg-search" type="search" placeholder="Search…" />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="sg-disabled">Disabled</Label>
              <Input id="sg-disabled" disabled defaultValue="Unavailable" />
            </div>
            <div className="grid gap-1.5">
              <Label>Select</Label>
              <Select value={selectValue} onValueChange={setSelectValue}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="Choose provider" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="openai">OpenAI</SelectItem>
                  <SelectItem value="anthropic">Anthropic</SelectItem>
                  <SelectItem value="typesafe">TypeSafe</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <label className="check-row">
              <Checkbox checked={checked} onCheckedChange={(value) => setChecked(value === true)} />
              <span>Checkbox label</span>
            </label>
          </CardContent>
        </Card>
      </Section>

      <Section id="alerts" title="Alerts" description="Inline status and error banners.">
        <div className="grid gap-2">
          <Alert>
            <AlertTitle>Default alert</AlertTitle>
            <AlertDescription>Use for neutral operator notices.</AlertDescription>
          </Alert>
          <Alert variant="destructive">
            <AlertTitle>Destructive alert</AlertTitle>
            <AlertDescription>Use for failed requests and blocked actions.</AlertDescription>
          </Alert>
        </div>
      </Section>

      <Section id="cards" title="Cards" description="Default and compact surface containers.">
        <div className="grid gap-3 sm:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle>Default card</CardTitle>
              <CardDescription>Title, description, content, and footer.</CardDescription>
            </CardHeader>
            <CardContent>
              Card body content goes here.
            </CardContent>
            <CardFooter>
              <Button size="sm">Action</Button>
            </CardFooter>
          </Card>
          <Card size="sm">
            <CardContent className="grid gap-1">
              <div className="text-xs text-muted-foreground">Stat card</div>
              <div className="font-mono text-base font-semibold">$12.40</div>
            </CardContent>
          </Card>
        </div>
      </Section>

      <Section id="table" title="Table" description="Dense data tables for lists and logs.">
        <Card>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Value</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow>
                  <TableCell className="font-medium">alice</TableCell>
                  <TableCell><Badge>active</Badge></TableCell>
                  <TableCell className="font-mono">$4.20</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell className="font-medium">bob</TableCell>
                  <TableCell><Badge variant="secondary">idle</Badge></TableCell>
                  <TableCell className="font-mono">$0.00</TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      </Section>

      <Section id="tooltip" title="Tooltip" description="Short help for non-obvious controls.">
        <Card>
          <CardContent className="flex flex-wrap items-center gap-4">
            <span className="inline-flex items-center gap-1">
              Enforce
              <InfoTip label="Enforce help">
                Enforced budgets reject requests at the limit.
              </InfoTip>
            </span>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="outline" size="sm">Hover me</Button>
              </TooltipTrigger>
              <TooltipContent>Tooltip on a button trigger.</TooltipContent>
            </Tooltip>
          </CardContent>
        </Card>
      </Section>

      <Section id="dialog" title="Dialog" description="Modal confirmations and create/edit forms.">
        <Card>
          <CardContent>
            <Button onClick={() => setDialogOpen(true)}>Open dialog</Button>
          </CardContent>
        </Card>
        <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Example dialog</DialogTitle>
              <DialogDescription>
                Use dialogs for create, edit, and destructive confirmations.
              </DialogDescription>
            </DialogHeader>
            <div className="grid gap-1.5">
              <Label htmlFor="sg-dialog-input">Name</Label>
              <Input id="sg-dialog-input" placeholder="alice-app" />
            </div>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setDialogOpen(false)}>Cancel</Button>
              <Button onClick={() => setDialogOpen(false)}>Save</Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </Section>

      <Section
        id="domain"
        title="Domain helpers"
        description="Gateway-specific presentation helpers used across pages."
      >
        <Card>
          <CardContent className="grid gap-3">
            <div className="flex flex-wrap items-center gap-2">
              <span className="status-live">Live status</span>
              <span className="status-disabled">Disabled status</span>
            </div>
            <div className="progress-wrap" style={{ maxWidth: 220 }}>
              <div className="progress-track">
                <div className="progress-fill" style={{ width: '62%' }} />
              </div>
              <span className="text-xs text-muted-foreground">62%</span>
            </div>
            <div className="bar-chart" style={{ maxWidth: 320 }}>
              <div className="bar-row">
                <span className="bar-label mono">03-10</span>
                <div className="bar-track"><div className="bar-fill" style={{ width: '80%' }} /></div>
                <span className="bar-value">$8.20</span>
              </div>
              <div className="bar-row">
                <span className="bar-label mono">03-11</span>
                <div className="bar-track"><div className="bar-fill" style={{ width: '45%' }} /></div>
                <span className="bar-value">$4.10</span>
              </div>
            </div>
          </CardContent>
        </Card>
      </Section>
    </>
  )
}
