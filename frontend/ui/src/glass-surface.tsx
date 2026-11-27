import { cva } from 'class-variance-authority'
import { createContext } from 'react'

export const GlassControlGroupContext = createContext(false)

export const glassSurfaceVariants = cva([
  'bg-surface-container-high rounded-full shadow-lg',
  'supports-[backdrop-filter]:bg-surface-container-high/76 supports-[backdrop-filter]:backdrop-blur-xl',
  'motion-reduce:backdrop-blur-none',
  '[@media(prefers-reduced-transparency:reduce)]:bg-surface-container-high',
  'contrast-more:bg-surface-container-high',
  'border border-outline-variant/40',
  'shadow-shadow/15 dark:shadow-shadow/40',
])
