import { cva } from 'class-variance-authority'
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

// In the page's two-column grid each section lays its heading and its card
// on the grid's own rows, so headings and cards line up across columns
// whatever the length of their descriptions. A `tall` section's card runs
// down beside two stacked sections.
const themeSectionVariants = cva('@3xl:grid @3xl:grid-rows-subgrid', {
  variants: {
    span: {
      short: '@3xl:row-span-2',
      tall: '@3xl:row-span-5',
    },
  },
  defaultVariants: {
    span: 'short',
  },
})

const themeSectionGroupVariants = cva('flex-1', {
  variants: {
    span: {
      short: '',
      tall: '@3xl:row-span-4',
    },
  },
  defaultVariants: {
    span: 'short',
  },
})

/** A titled settings section whose one item holds free-form controls. */
export function ThemeSection({
  title,
  description,
  span,
  className,
  children,
  'data-slot': slot,
}: {
  title: string
  description?: string
  span?: 'short' | 'tall'
  className?: string
  children: ReactNode
  'data-slot': string
}) {
  return (
    <ItemSection
      data-slot={slot}
      className={cn(themeSectionVariants({ span }), className)}
    >
      <div data-slot="theme-section-heading">
        <ItemSectionHeader>
          <ItemSectionTitle>{title}</ItemSectionTitle>
        </ItemSectionHeader>

        {description && (
          <ItemSectionDescription>{description}</ItemSectionDescription>
        )}
      </div>

      {/* A section stretched by the grid grows its item to match. */}
      <ItemGroup className={themeSectionGroupVariants({ span })}>
        <Item className="flex-1 flex-col items-stretch">{children}</Item>
      </ItemGroup>
    </ItemSection>
  )
}
