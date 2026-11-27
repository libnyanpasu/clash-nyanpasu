import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { AnimatePresence, motion } from 'motion/react'
import { ComponentProps, ReactNode, useId } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { useScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { useIsMobile } from '@nyanpasu/hooks'
import { cn } from '@nyanpasu/utils'
import { Link, LinkProps } from '@tanstack/react-router'
import { useCrumbParent, useLeafCrumb } from './router/route-crumbs'

export const BackButton = ({
  to,
  params,
  label,
  search,
  className,
  variant = 'raised',
  ...props
}: {
  to?: LinkProps['to']
  params?: LinkProps['params']
  /** The accessible name of the link, since it shows only an arrow. */
  label?: string
  search?: LinkProps['search']
} & Omit<ComponentProps<typeof Button>, 'asChild' | 'children' | 'icon'>) => {
  const parent = useCrumbParent()
  const isMobile = useIsMobile()

  const target = to ?? parent?.to

  if (!target || (to === undefined && parent?.hub && !isMobile)) {
    return null
  }

  const targetParams = to ? params : parent?.params
  const accessibleLabel =
    label ??
    (parent ? m.navigation_return_to({ page: parent.title }) : m.profile_back())

  return (
    <Button
      icon
      variant={variant}
      className={cn(
        'flex items-center justify-center',
        to !== undefined && 'md:hidden',
        className,
      )}
      asChild
      {...props}
    >
      <Link
        to={target as LinkProps['to']}
        params={targetParams as LinkProps['params']}
        search={search}
        aria-label={accessibleLabel}
      >
        <ArrowBackIosNewRounded className="size-4" />
      </Link>
    </Button>
  )
}

const Title = (props: ComponentProps<typeof motion.p>) => {
  return (
    <motion.p
      layout
      layoutCrossfade={false}
      transition={{
        layout: {
          duration: 0.5,
          ease: [0.32, 0.72, 0, 1],
        },
        opacity: {
          duration: 0.16,
        },
      }}
      {...props}
    />
  )
}

export function PageTitle({
  title,
  actions,
  className,
  ...props
}: ComponentProps<'div'> & {
  /** Overrides the crumb title for a name that only the loaded data knows. */
  title?: ReactNode
  /** Controls at the end of the sticky bar, which stay while the page scrolls. */
  actions?: ReactNode
}) {
  const { isPastHeader } = useScrollArea()
  const leaf = useLeafCrumb()

  const id = useId()

  const showTopTitle = isPastHeader
  const shownTitle =
    title ?? (leaf ? leaf.crumb.title({ params: leaf.params }) : null)

  return (
    <>
      <div
        className={cn(
          'group sticky top-0 z-10',
          'bg-surface-container-low dark:bg-surface',
          'flex items-center gap-4',
          'h-16 px-4 md:px-6',
          className,
        )}
        data-show-title={showTopTitle}
        data-slot="page-title"
        {...props}
      >
        <BackButton />

        <AnimatePresence initial={false}>
          {showTopTitle && (
            <Title
              key="page-title-top"
              layoutId={id}
              className="text-xl font-bold"
            >
              {shownTitle}
            </Title>
          )}
        </AnimatePresence>

        {actions && (
          <div className="ml-auto flex shrink-0 items-center gap-1">
            {actions}
          </div>
        )}
      </div>

      <div
        className="group relative z-20 flex h-24 px-6 pt-10 pb-4"
        data-slot="page-title"
        data-show-title={!showTopTitle}
      >
        <AnimatePresence initial={false}>
          {!showTopTitle && (
            <Title
              key="page-title-main"
              layoutId={id}
              className="text-3xl font-bold"
            >
              {shownTitle}
            </Title>
          )}
        </AnimatePresence>
      </div>
    </>
  )
}
