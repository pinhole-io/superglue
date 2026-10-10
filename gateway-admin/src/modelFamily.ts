/** Classify gateway model ids into System 1, System 2, embeddings, or other. */

export type ModelFamily = 'system1' | 'system2' | 'embeddings' | 'other'

export const SYSTEM2_PROVIDERS = [
  'openai',
  'anthropic',
  'xai',
  'groq',
  'openrouter',
  'runinfra',
  'vercel',
] as const

export type System2Provider = (typeof SYSTEM2_PROVIDERS)[number]

/** OpenAI-compatible providers that support embeddings. */
export const EMBEDDING_PROVIDERS = [
  'openai',
  'xai',
  'groq',
  'openrouter',
  'runinfra',
  'vercel',
] as const

export type EmbeddingProvider = (typeof EMBEDDING_PROVIDERS)[number]

export const DEFAULT_SYSTEM2_MODELS = ['openai:*', 'anthropic:*'] as const

export const FAMILY_LABEL: Record<ModelFamily, string> = {
  system1: 'System 1',
  system2: 'System 2',
  embeddings: 'Embeddings',
  other: 'Other',
}

export const FAMILY_DESCRIPTION: Record<Exclude<ModelFamily, 'other'>, string> = {
  system1: 'TypeSafe evaluation via POST /v1/systemone. These models do not answer chat.',
  system2: 'Chat and Responses via POST /v1/chat/completions and POST /v1/responses.',
  embeddings: 'Vector models via POST /v1/embeddings. The embedding: tag is required.',
}

export function providerOf(id: string): string | null {
  const colon = id.indexOf(':')
  if (colon <= 0) return null
  return id.slice(0, colon).toLowerCase()
}

export function classifyModel(id: string): ModelFamily {
  const trimmed = id.trim()
  if (!trimmed) return 'other'
  if (trimmed.includes(':embedding:')) return 'embeddings'
  const provider = providerOf(trimmed)
  if (provider === 'typesafe') return 'system1'
  if (provider && (SYSTEM2_PROVIDERS as readonly string[]).includes(provider)) return 'system2'
  return 'other'
}

export function isSystem2Provider(provider: string): provider is System2Provider {
  return (SYSTEM2_PROVIDERS as readonly string[]).includes(provider)
}

export function isEmbeddingProvider(provider: string): provider is EmbeddingProvider {
  return (EMBEDDING_PROVIDERS as readonly string[]).includes(provider)
}

export function system2Wildcard(provider: System2Provider): string {
  return `${provider}:*`
}

export function embeddingWildcard(provider: EmbeddingProvider): string {
  return `${provider}:embedding:*`
}

export function groupByFamily(ids: string[]): Record<ModelFamily, string[]> {
  const groups: Record<ModelFamily, string[]> = {
    system1: [],
    system2: [],
    embeddings: [],
    other: [],
  }
  for (const id of ids) {
    groups[classifyModel(id)].push(id)
  }
  return groups
}

export function providerLabel(provider: string): string {
  const labels: Record<string, string> = {
    openai: 'OpenAI',
    anthropic: 'Anthropic',
    xai: 'xAI',
    groq: 'Groq',
    openrouter: 'OpenRouter',
    runinfra: 'RunInfra',
    vercel: 'Vercel',
    typesafe: 'TypeSafe',
  }
  return labels[provider] ?? provider
}

export function providerFamily(providerId: string): Exclude<ModelFamily, 'other' | 'embeddings'> {
  return providerId === 'typesafe' ? 'system1' : 'system2'
}

export function providerTooltip(providerId: string): string {
  switch (providerId) {
    case 'typesafe':
      return 'System 1 only. Calls POST /v1/systemone. Does not support chat or embeddings.'
    case 'anthropic':
      return 'System 2 chat and Responses. Does not support embeddings.'
    case 'openai':
    case 'groq':
      return 'System 2 chat and Responses. Also supports POST /v1/audio/transcriptions.'
    default:
      return 'System 2 chat and Responses. OpenAI-compatible providers also support embeddings when tagged with embedding:.'
  }
}
