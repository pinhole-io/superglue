import { useEffect, useMemo, useState } from 'react'
import { api } from '../api'
import {
  DEFAULT_SYSTEM2_MODELS,
  EMBEDDING_PROVIDERS,
  FAMILY_DESCRIPTION,
  SYSTEM2_PROVIDERS,
  classifyModel,
  embeddingWildcard,
  isEmbeddingProvider,
  isSystem2Provider,
  providerLabel,
  providerOf,
  system2Wildcard,
  type EmbeddingProvider,
  type System2Provider,
} from '../modelFamily'
import { InfoTip } from './InfoTip'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Button } from '@/components/ui/button'

function toggle(list: string[], id: string, on: boolean): string[] {
  if (on) return list.includes(id) ? list : [...list, id]
  return list.filter((item) => item !== id)
}

function SectionHeading({
  title,
  tipLabel,
  tip,
  description,
}: {
  title: string
  tipLabel: string
  tip: string
  description: string
}) {
  return (
    <div className="model-section-heading">
      <div className="flex items-center gap-1.5">
        <h3>{title}</h3>
        <InfoTip label={tipLabel}>{tip}</InfoTip>
      </div>
      <p>{description}</p>
    </div>
  )
}

function CheckRow({
  id,
  label,
  checked,
  tip,
  tipLabel,
  onChange,
}: {
  id: string
  label: string
  checked: boolean
  tip?: string
  tipLabel?: string
  onChange: (checked: boolean) => void
}) {
  return (
    <label className="check-row" htmlFor={id}>
      <Checkbox
        id={id}
        checked={checked}
        onCheckedChange={(value) => onChange(value === true)}
      />
      <span className="mono">{label}</span>
      {tip && tipLabel ? <InfoTip label={tipLabel}>{tip}</InfoTip> : null}
    </label>
  )
}

export function ModelPicker({
  selected,
  useDefault,
  hideDefaultOption,
  onChange,
}: {
  selected: string[]
  useDefault: boolean
  hideDefaultOption?: boolean
  onChange: (models: string[], useDefault: boolean) => void
}) {
  const [catalog, setCatalog] = useState<string[]>([])
  const [searchByProvider, setSearchByProvider] = useState<Record<string, string>>({})
  const [embeddingDraft, setEmbeddingDraft] = useState({ provider: 'openai' as EmbeddingProvider, model: '' })

  useEffect(() => {
    api
      .models()
      .then((result) => setCatalog(result.data.map((model) => model.id)))
      .catch(() => setCatalog([]))
  }, [])

  const effective = useDefault && !hideDefaultOption ? [...DEFAULT_SYSTEM2_MODELS] : selected
  const otherPatterns = useMemo(
    () => effective.filter((id) => classifyModel(id) === 'other'),
    [effective],
  )

  const system1Catalog = catalog.filter((id) => classifyModel(id) === 'system1')
  const system2ByProvider = useMemo(() => {
    const map = Object.fromEntries(SYSTEM2_PROVIDERS.map((p) => [p, [] as string[]])) as Record<
      System2Provider,
      string[]
    >
    for (const id of catalog) {
      if (classifyModel(id) !== 'system2') continue
      const provider = providerOf(id)
      if (!provider || !isSystem2Provider(provider)) continue
      if (id.endsWith(':*')) continue
      map[provider].push(id)
    }
    return map
  }, [catalog])

  const embeddingCatalog = catalog.filter((id) => classifyModel(id) === 'embeddings')

  const setSelected = (next: string[]) => onChange(next, false)

  const addEmbedding = () => {
    const model = embeddingDraft.model.trim()
    if (!model || model.includes(':')) return
    const id = `${embeddingDraft.provider}:embedding:${model}`
    setSelected(toggle(effective, id, true))
    setEmbeddingDraft((current) => ({ ...current, model: '' }))
  }

  return (
    <div className="model-picker">
      {!hideDefaultOption && (
        <label className="check-row">
          <Checkbox
            checked={useDefault}
            onCheckedChange={(checked) => onChange([], checked === true)}
          />
          <span>Use default System 2 models (`openai:*`, `anthropic:*`)</span>
          <InfoTip label="Default models">
            Grants OpenAI and Anthropic chat wildcards only. System 1 and embeddings stay off.
          </InfoTip>
        </label>
      )}

      {(hideDefaultOption || !useDefault) && (
        <div className="model-family-stack">
          <section className="model-section">
            <SectionHeading
              title="System 1"
              tipLabel="System 1"
              tip={FAMILY_DESCRIPTION.system1}
              description="TypeSafe evaluation models."
            />
            {system1Catalog.length === 0 && !effective.some((id) => classifyModel(id) === 'system1') ? (
              <p className="muted">No TypeSafe models in the catalog. Configure TYPESAFE_API_KEY on the gateway.</p>
            ) : (
              <div className="model-grid">
                <CheckRow
                  id="model-typesafe-star"
                  label="typesafe:*"
                  checked={effective.includes('typesafe:*')}
                  tip="All TypeSafe System 1 models."
                  tipLabel="typesafe wildcard"
                  onChange={(on) => setSelected(toggle(effective, 'typesafe:*', on))}
                />
                {system1Catalog.map((id) => (
                  <CheckRow
                    key={id}
                    id={`model-${id}`}
                    label={id}
                    checked={effective.includes(id)}
                    onChange={(on) => setSelected(toggle(effective, id, on))}
                  />
                ))}
              </div>
            )}
          </section>

          <section className="model-section">
            <SectionHeading
              title="System 2"
              tipLabel="System 2"
              tip={FAMILY_DESCRIPTION.system2}
              description="Chat and Responses providers. OpenAI and Groq also cover transcription."
            />
            <div className="provider-blocks">
              {SYSTEM2_PROVIDERS.map((provider) => {
                const wildcard = system2Wildcard(provider)
                const models = system2ByProvider[provider]
                const query = (searchByProvider[provider] ?? '').trim().toLowerCase()
                const filtered = query
                  ? models.filter((id) => id.toLowerCase().includes(query))
                  : models
                return (
                  <div className="provider-block" key={provider}>
                    <div className="provider-block-header">
                      <strong>{providerLabel(provider)}</strong>
                      <CheckRow
                        id={`model-${wildcard}`}
                        label={wildcard}
                        checked={effective.includes(wildcard)}
                        tip={`All ${providerLabel(provider)} chat models.`}
                        tipLabel={`${provider} wildcard`}
                        onChange={(on) => setSelected(toggle(effective, wildcard, on))}
                      />
                    </div>
                    {models.length > 0 && (
                      <>
                        <Input
                          type="search"
                          placeholder={`Search ${providerLabel(provider)} models…`}
                          value={searchByProvider[provider] ?? ''}
                          onChange={(event) =>
                            setSearchByProvider((current) => ({
                              ...current,
                              [provider]: event.target.value,
                            }))
                          }
                        />
                        <div className="model-grid">
                          {filtered.length === 0 ? (
                            <span className="muted">No models match this search.</span>
                          ) : (
                            filtered.map((id) => (
                              <CheckRow
                                key={id}
                                id={`model-${id}`}
                                label={id}
                                checked={effective.includes(id)}
                                onChange={(on) => setSelected(toggle(effective, id, on))}
                              />
                            ))
                          )}
                        </div>
                      </>
                    )}
                  </div>
                )
              })}
            </div>
          </section>

          <section className="model-section">
            <SectionHeading
              title="Embeddings"
              tipLabel="Embeddings"
              tip={FAMILY_DESCRIPTION.embeddings}
              description="Only OpenAI-compatible providers. Anthropic and TypeSafe have no embeddings group."
            />
            <div className="model-grid">
              {EMBEDDING_PROVIDERS.map((provider) => {
                const wildcard = embeddingWildcard(provider)
                return (
                  <CheckRow
                    key={wildcard}
                    id={`model-${wildcard}`}
                    label={wildcard}
                    checked={effective.includes(wildcard)}
                    tip={`All ${providerLabel(provider)} embedding models. The embedding: tag is required.`}
                    tipLabel={`${provider} embedding wildcard`}
                    onChange={(on) => setSelected(toggle(effective, wildcard, on))}
                  />
                )
              })}
              {embeddingCatalog.map((id) => (
                <CheckRow
                  key={id}
                  id={`model-${id}`}
                  label={id}
                  checked={effective.includes(id)}
                  onChange={(on) => setSelected(toggle(effective, id, on))}
                />
              ))}
            </div>
            <div className="embedding-add">
              <Label htmlFor="embedding-provider">Add tagged embedding model</Label>
              <div className="embedding-add-row">
                <select
                  id="embedding-provider"
                  value={embeddingDraft.provider}
                  onChange={(event) => {
                    const provider = event.target.value
                    if (isEmbeddingProvider(provider)) {
                      setEmbeddingDraft((current) => ({ ...current, provider }))
                    }
                  }}
                >
                  {EMBEDDING_PROVIDERS.map((provider) => (
                    <option key={provider} value={provider}>
                      {providerLabel(provider)}
                    </option>
                  ))}
                </select>
                <Input
                  placeholder="text-embedding-3-small"
                  value={embeddingDraft.model}
                  onChange={(event) =>
                    setEmbeddingDraft((current) => ({ ...current, model: event.target.value }))
                  }
                />
                <Button type="button" variant="outline" onClick={addEmbedding} disabled={!embeddingDraft.model.trim()}>
                  Add
                </Button>
              </div>
              <p className="muted">
                {`Stores as ${embeddingDraft.provider}:embedding:<id>. The UI does not infer the tag from a bare model name.`}
              </p>
            </div>
          </section>

          {otherPatterns.length > 0 && (
            <section className="model-section">
              <SectionHeading
                title="Other patterns"
                tipLabel="Other patterns"
                tip="These patterns are not System 1, System 2, or embeddings. Keep them only if you need them."
                description="Selected ids that the classifier does not recognize."
              />
              <div className="model-grid">
                {otherPatterns.map((id) => (
                  <CheckRow
                    key={id}
                    id={`model-other-${id}`}
                    label={id}
                    checked
                    onChange={(on) => setSelected(toggle(effective, id, on))}
                  />
                ))}
              </div>
            </section>
          )}
        </div>
      )}
    </div>
  )
}
