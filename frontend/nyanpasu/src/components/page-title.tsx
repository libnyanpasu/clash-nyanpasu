import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { AnimatePresence, motion } from 'motion/react'
import { ComponentProps, useId } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { useScrollArea } from '@nyanpasu/ui/scroll-area'
import { cn } from '@nyanpasu/utils'
import { Link, LinkProps } from '@tanstack/react-router'

const BackButton = ({ to }: { to: LinkProps['to'] }) => {
  return (
    <Button
      icon
      variant="raised"
      className="flex items-center justify-center md:hidden"
      asChild
    >
      <Link to={to}>
        <ArrowBackIosNewRounded className="size-4" />
      </Link>
    </Button>
  )
}

const Title = (props: ComponentProps<typeof motion.p>) => {
  return (
    <motion.p
      layout
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
  backTo,
  className,
  children,
  ...props
}: ComponentProps<'div'> & {
  /** Route the mobile back button navigates to. */
  backTo: LinkProps['to']
}) {
  const { isPastHeader } = useScrollArea()

  const id = useId()

  const showTopTitle = isPastHeader

  return (
    <>
      <div
        className={cn(
          'group sticky top-0 z-10',
          'bg-mixed-background',
          'flex items-center gap-4',
          'h-16 px-4 md:px-6',
          className,
        )}
        data-show-title={showTopTitle}
        data-slot="page-title"
        {...props}
      >
        <BackButton to={backTo} />

        <AnimatePresence initial={false}>
          {showTopTitle && (
            <Title
              key="page-title-top"
              layoutId={id}
              className="text-xl font-bold"
            >
              {children}
            </Title>
          )}
        </AnimatePresence>
      </div>

      <div
        className="group flex h-24 px-6 pt-10 pb-4"
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
              {children}
            </Title>
          )}
        </AnimatePresence>
      </div>
    </>
  )
}
