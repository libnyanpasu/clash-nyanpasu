import { AnimatePresence, motion, Variants } from 'framer-motion'
import { CSSProperties } from 'react'
import LogoSvg from '@/assets/image/logo.svg?react'
import { useNyanpasu } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import styles from './animated-logo.module.scss'

const Logo = motion.create(LogoSvg)

const transition = {
  type: 'spring',
  stiffness: 260,
  damping: 20,
}

const motionVariants: { [name: string]: Variants } = {
  default: {
    initial: {
      opacity: 0,
      scale: 0.5,
      transition,
    },
    animate: {
      opacity: 1,
      scale: 1,
      transition,
    },
    exit: {
      opacity: 0,
      scale: 0.5,
      transition,
    },
    whileHover: {
      scale: 1.1,
      transition,
    },
  },
  none: {
    initial: {},
    animate: {},
    exit: {},
  },
}

export default function AnimatedLogo({
  className,
  style,
  disableMotion,
}: {
  className?: string
  style?: CSSProperties
  disableMotion?: boolean
}) {
  const { nyanpasuConfig } = useNyanpasu()

  const disable = disableMotion ?? nyanpasuConfig?.lighten_animation_effects

  return (
    <AnimatePresence initial={false}>
      <Logo
        className={cn(styles.LogoSchema, className)}
        variants={motionVariants[disable ? 'none' : 'default']}
        style={style}
        drag
        dragConstraints={{ left: 0, right: 0, top: 0, bottom: 0 }}
        whileDrag={{ scale: 1.15 }}
        dragSnapToOrigin
        dragElastic={0.1}
      />
    </AnimatePresence>
  )
}
