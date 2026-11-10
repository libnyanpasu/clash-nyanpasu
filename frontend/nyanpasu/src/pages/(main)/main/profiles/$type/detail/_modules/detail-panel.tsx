import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import { cva } from 'class-variance-authority'
import { useEffect, useId, useRef, type ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'

/** The panels a profile's detail page opens beside its rows. */
export const DETAIL_PANELS = ['transforms', 'sync'] as const

export type DetailPanelKind = (typeof DETAIL_PANELS)[number]

/**
 * Whether the content area fits the detail column and the panel side by side
 * (`wide`), or the panel takes the place of the column (`narrow`).
 */
export type DetailPanelLayout = 'wide' | 'narrow'

const detailPanelVariants = cva(
  'bg-mixed-background flex min-h-0 min-w-0 flex-col outline-none',
  {
    variants: {
      layout: {
        wide: [
          'w-[26rem] shrink-0',
          'shadow-[inset_1px_0_0_var(--color-outline-variant)]',
        ],
        narrow: 'flex-1',
      },
    },
  },
)

/**
 * A labelled region for one panel of the detail page, with its own scrolling.
 * It takes focus when it opens. The wide layout closes it with an icon button;
 * the narrow one, where it replaces the page, gives it a back button and names
 * the profile under the title.
 */
export function DetailPanel({
  title,
  profileName,
  layout,
  onClose,
  children,
}: {
  title: string
  profileName: string
  layout: DetailPanelLayout
  /** Returns to the detail page. */
  onClose: () => void
  children: ReactNode
}) {
  const titleId = useId()

  const regionRef = useRef<HTMLElement>(null)

  useEffect(() => {
    regionRef.current?.focus()
  }, [])

  const Title = layout === 'narrow' ? 'h1' : 'h2'

  return (
    <section
      ref={regionRef}
      tabIndex={-1}
      aria-labelledby={titleId}
      data-slot="profile-detail-panel"
      data-layout={layout}
      className={detailPanelVariants({ layout })}
    >
      <header
        data-slot="profile-detail-panel-header"
        className="flex min-h-14 shrink-0 items-center gap-2 py-2 pr-3 pl-4"
      >
        {layout === 'narrow' && (
          <Button
            icon
            className="shrink-0"
            aria-label={m.profile_back()}
            data-slot="profile-detail-panel-back"
            onClick={onClose}
          >
            <ArrowBackIosNewRounded className="size-4" aria-hidden />
          </Button>
        )}

        <div className="flex min-w-0 flex-1 flex-col">
          <Title
            id={titleId}
            className="text-on-surface truncate text-base leading-6 font-semibold"
          >
            {title}
          </Title>

          {layout === 'narrow' && (
            <p
              data-slot="profile-detail-panel-subtitle"
              className="text-on-surface-variant truncate text-xs leading-4"
            >
              {profileName}
            </p>
          )}
        </div>

        {layout === 'wide' && (
          <Button
            icon
            className="shrink-0"
            aria-label={m.common_close()}
            data-slot="profile-detail-panel-close"
            onClick={onClose}
          >
            <CloseRounded className="size-5" aria-hidden />
          </Button>
        )}
      </header>

      <ScrollArea
        data-slot="profile-detail-panel-scroll-area"
        className="min-h-0 flex-1"
      >
        <div className="flex min-w-0 flex-col gap-4 px-3 pt-1 pb-6">
          {children}
        </div>
      </ScrollArea>
    </section>
  )
}
