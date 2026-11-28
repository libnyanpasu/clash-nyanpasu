import type { ReactNode } from 'react'
import {
  Item,
  ItemGroup,
  ItemSection,
  ItemSectionDescription,
  ItemSectionHeader,
  ItemSectionTitle,
} from '@nyanpasu/ui/item-group'

/** A titled settings section whose one item holds free-form controls. */
export function ThemeSection({
  title,
  description,
  action,
  children,
  'data-slot': slot,
}: {
  title: string
  description?: string
  action?: ReactNode
  children: ReactNode
  'data-slot': string
}) {
  return (
    <ItemSection data-slot={slot}>
      <ItemSectionHeader>
        <ItemSectionTitle>{title}</ItemSectionTitle>

        {action}
      </ItemSectionHeader>

      {description && (
        <ItemSectionDescription>{description}</ItemSectionDescription>
      )}

      <ItemGroup>
        <Item className="flex-col items-stretch">{children}</Item>
      </ItemGroup>
    </ItemSection>
  )
}
