import type { ReactElement } from 'react'
import { SharedTooltipTrigger } from '@nyanpasu/ui/shared-tooltip'

/** A card action whose label shows in the profiles pages' shared tooltip. */
export default function ProfileCardAction({
  label,
  children,
}: {
  label: string
  children: ReactElement
}) {
  return <SharedTooltipTrigger content={label}>{children}</SharedTooltipTrigger>
}
