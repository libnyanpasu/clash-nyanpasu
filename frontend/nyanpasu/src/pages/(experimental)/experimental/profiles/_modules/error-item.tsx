import { motion } from 'framer-motion'
import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/ui'

export default function AnimatedErrorItem({
  className,
  ...props
}: ComponentProps<typeof motion.div>) {
  return (
    <motion.div
      className={cn('overflow-hidden', className)}
      initial={{
        height: 0,
        opacity: 0,
      }}
      animate={{
        height: 'auto',
        opacity: 1,
      }}
      exit={{
        height: 0,
        opacity: 0,
      }}
      transition={{
        height: {
          duration: 0.2,
          ease: 'easeInOut',
        },
        opacity: {
          duration: 0.15,
        },
      }}
      {...props}
    />
  )
}
