import { createColorize } from 'colorize-template'
import { createConsola } from 'consola'
import pc from 'picocolors'

export const consola = createConsola({
  level: process.env.LOG_LEVEL ? Number.parseInt(process.env.LOG_LEVEL) : 5,
  fancy: true,
  formatOptions: {
    colors: true,
    compact: false,
    date: true,
  },
})

export const colorize = createColorize({
  ...pc,
  success: pc.green,
  error: pc.red,
})
