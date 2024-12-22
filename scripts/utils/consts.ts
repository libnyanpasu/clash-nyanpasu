import { execSync } from 'child_process'

export const SIDECAR_HOST: string | undefined = process.argv.includes(
  '--sidecar-host',
)
  ? process.argv[process.argv.indexOf('--sidecar-host') + 1]
  : execSync('rustc -vV')
      .toString()
      ?.match(/(?<=host: ).+(?=\s*)/g)?.[0]
