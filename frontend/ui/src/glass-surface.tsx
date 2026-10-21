import { cva } from 'class-variance-authority'
import { createContext } from 'react'

export const GlassControlGroupContext = createContext(false)

export const glassSurfaceVariants = cva([
  'bg-surface-variant rounded-full shadow-lg',
  'shadow-shadow/15 dark:shadow-shadow/40',
])
