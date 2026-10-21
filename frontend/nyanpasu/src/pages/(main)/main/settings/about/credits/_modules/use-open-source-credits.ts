import { useQuery } from '@tanstack/react-query'

export type CreditSource = 'bundled' | 'cargo' | 'npm'

export type CreditPackage = {
  source: CreditSource
  name: string
  version?: string
  homepage?: string
  /** SPDX expression; absent for bundled components without a license. */
  license?: string
  /** Keys of the package's texts in {@link OpenSourceCredits.texts}. */
  textIds: string[]
  kind?: 'sidecar' | 'resource' | 'data'
  platforms?: ('windows' | 'linux' | 'macos')[]
  notes?: string
}

/**
 * Written by `deno task generate:credits`
 * (scripts/src/generate/credits/document.ts) before every app build.
 */
export type OpenSourceCredits = {
  schemaVersion: 1
  texts: Record<string, string>
  packages: CreditPackage[]
}

// The file is generated, not committed: a development server without it
// matches nothing here instead of failing to resolve an import.
const creditsModules = import.meta.glob<OpenSourceCredits>(
  '@/assets/credits/open-source-credits.json',
  { import: 'default' },
)

/** Loads the credits on first use; null when this build has none. */
export function useOpenSourceCredits() {
  return useQuery({
    queryKey: ['open-source-credits'],
    queryFn: async () => {
      const [load] = Object.values(creditsModules)

      return load ? await load() : null
    },
    staleTime: Infinity,
  })
}
