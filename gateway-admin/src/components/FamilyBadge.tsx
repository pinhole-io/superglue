import { Badge } from '@/components/ui/badge'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import {
  FAMILY_DESCRIPTION,
  FAMILY_LABEL,
  classifyModel,
  type ModelFamily,
} from '../modelFamily'

const VARIANT: Record<ModelFamily, 'default' | 'secondary' | 'outline' | 'destructive'> = {
  system1: 'default',
  system2: 'secondary',
  embeddings: 'outline',
  other: 'destructive',
}

export function FamilyBadge({ id, family }: { id?: string; family?: ModelFamily }) {
  const resolved = family ?? (id ? classifyModel(id) : 'other')
  const tip =
    resolved === 'other'
      ? 'This pattern does not match a known family. Remove it if it is unused.'
      : FAMILY_DESCRIPTION[resolved]

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge variant={VARIANT[resolved]}>{FAMILY_LABEL[resolved]}</Badge>
      </TooltipTrigger>
      <TooltipContent>{tip}</TooltipContent>
    </Tooltip>
  )
}
