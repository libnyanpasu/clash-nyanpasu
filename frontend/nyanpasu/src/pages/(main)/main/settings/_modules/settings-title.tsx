import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import { ComponentProps, useId } from 'react'
import { Button } from '@/components/ui/button'
import { useScrollArea } from '@/components/ui/scroll-area'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'

const BackButton = () => {
  return (
    <Button
      icon
      variant="raised"
      className="flex items-center justify-center md:hidden"
      asChild
    >
      <Link to="/main/settings">
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

export function SettingsTitle({
  className,
  children,
  ...props
}: ComponentProps<'div'>) {
  const { offset } = useScrollArea()

  const id = useId()

  const showTopTitle = offset.top > 40

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
        data-slot="settings-title"
        {...props}
      >
        <BackButton />

        <AnimatePresence initial={false}>
          {showTopTitle && (
            <Title
              key="settings-title-top"
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
        data-slot="settings-title"
        data-show-title={!showTopTitle}
      >
        <AnimatePresence initial={false}>
          {!showTopTitle && (
            <Title
              key="settings-title-main"
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
