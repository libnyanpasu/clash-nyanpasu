import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import { cva } from 'class-variance-authority'
import { motion, useReducedMotion, type Variants } from 'motion/react'
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

const detailPanelSlotVariants = cva('flex min-h-0', {
  variants: {
    layout: {
      // Clips the panel while the slot grows to its width.
      wide: 'shrink-0 overflow-hidden',
      narrow: 'absolute inset-0 z-10',
    },
  },
})

// The wide slot grows from nothing, which pushes the detail column aside; the
// narrow one slides over the column the way a routed page does. A whole
// `transform` value runs off the main thread, and the settled panel drops it.
const PANEL_SLIDE = {
  wide: {
    hidden: { width: 0, opacity: 0 },
    visible: { width: 'auto', opacity: 1 },
  },
  narrow: {
    hidden: { transform: 'translateX(30%)', opacity: 0 },
    visible: {
      transform: 'translateX(0%)',
      opacity: 1,
      transitionEnd: { transform: 'none' },
    },
  },
} satisfies Record<DetailPanelLayout, Variants>

/**
 * The place of the open panel in the detail page. Render it in an
 * `AnimatePresence` so the panel slides in when it opens and out when it
 * closes; the panels themselves swap inside it.
 */
export function DetailPanelSlot({
  layout,
  children,
}: {
  layout: DetailPanelLayout
  children: ReactNode
}) {
  const reduceMotion = useReducedMotion()

  return (
    <motion.div
      data-slot="profile-detail-panel-slot"
      className={detailPanelSlotVariants({ layout })}
      variants={PANEL_SLIDE[layout]}
      initial="hidden"
      animate="visible"
      exit="hidden"
      transition={
        reduceMotion
          ? { duration: 0 }
          : { type: 'spring', bounce: 0, duration: 0.35 }
      }
    >
      {children}
    </motion.div>
  )
}

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
    <motion.section
      ref={regionRef}
      tabIndex={-1}
      aria-labelledby={titleId}
      data-slot="profile-detail-panel"
      data-layout={layout}
      className={detailPanelVariants({ layout })}
      // A panel that takes the place of another one fades in.
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.2 }}
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
    </motion.section>
  )
}
