import { createColorize } from 'npm:colorize-template'
import { createConsola } from 'npm:consola'
import pc from 'npm:picocolors'

const logLevelStr = Deno.env.get('LOG_LEVEL')

export const consola = createConsola({
  level: logLevelStr ? Number.parseInt(logLevelStr) : 5,
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
