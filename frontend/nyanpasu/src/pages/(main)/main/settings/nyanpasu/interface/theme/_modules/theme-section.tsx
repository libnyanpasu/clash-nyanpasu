import type { ReactNode } from 'react'
import {
  Item,
  ItemGroup,
  ItemSection,
  ItemSectionDescription,
  ItemSectionHeader,
  ItemSectionTitle,
} from '@nyanpasu/ui/item-group'
import { cn } from '@nyanpasu/utils'

/** A titled settings section whose one item holds free-form controls. */
export function ThemeSection({
  title,
  description,
  action,
  className,
  children,
  'data-slot': slot,
}: {
  title: string
  description?: string
  action?: ReactNode
  className?: string
  children: ReactNode
  'data-slot': string
}) {
  return (
    <ItemSection data-slot={slot} className={className}>
      <ItemSectionHeader>
        <ItemSectionTitle>{title}</ItemSectionTitle>

        {action}
      </ItemSectionHeader>

      {description && (
        <ItemSectionDescription>{description}</ItemSectionDescription>
      )}

      {/* A section stretched by the grid grows its item to match. */}
      <ItemGroup className="flex-1">
        <Item className="flex-1 flex-col items-stretch">{children}</Item>
      </ItemGroup>
    </ItemSection>
  )
}
