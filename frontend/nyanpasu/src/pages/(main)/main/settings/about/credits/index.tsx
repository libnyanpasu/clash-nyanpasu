import { useDeferredValue, useMemo, useState } from 'react'
import { FilterChip } from '@nyanpasu/ui/chip'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { SearchField } from '@nyanpasu/ui/search-field'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'
import CreditCountBadge from './_modules/credit-count-badge'
import { CREDIT_SOURCES, creditSourceLabel } from './_modules/credit-labels'
import CreditLicenseModal from './_modules/credit-license-modal'
import CreditList from './_modules/credit-list'
import {
  useOpenSourceCredits,
  type CreditPackage,
  type CreditSource,
  type OpenSourceCredits,
} from './_modules/use-open-source-credits'

export const Route = createFileRoute('/(main)/main/settings/about/credits/')({
  component: RouteComponent,
})

function CreditsBrowser({ credits }: { credits: OpenSourceCredits }) {
  const [search, setSearch] = useState('')
  const deferredSearch = useDeferredValue(search)
  const [source, setSource] = useState<CreditSource | null>(null)

  // The last selected package stays while the dialog plays its exit.
  const [selected, setSelected] = useState<CreditPackage | null>(null)
  const [modalOpen, setModalOpen] = useState(false)

  const counts = useMemo(() => {
    const result: Record<CreditSource, number> = {
      bundled: 0,
      cargo: 0,
      npm: 0,
    }

    credits.packages.forEach((pkg) => result[pkg.source]++)

    return result
  }, [credits])

  const packages = useMemo(() => {
    const term = deferredSearch.trim().toLowerCase()

    return credits.packages.filter(
      (pkg) =>
        (source === null || pkg.source === source) &&
        (!term ||
          pkg.name.toLowerCase().includes(term) ||
          pkg.license?.toLowerCase().includes(term)),
    )
  }, [credits, source, deferredSearch])

  const handleSelect = (pkg: CreditPackage) => {
    setSelected(pkg)
    setModalOpen(true)
  }

  return (
    <>
      <div
        className="flex flex-col gap-3 sm:flex-row sm:items-center"
        data-slot="credits-toolbar"
      >
        <SearchField
          className="sm:max-w-80 sm:flex-1"
          value={search}
          onValueChange={setSearch}
          placeholder={m.settings_about_credits_search()}
          aria-label={m.settings_about_credits_search()}
          clearLabel={m.settings_about_credits_search_clear()}
        />

        <div className="flex flex-wrap gap-2" data-slot="credits-sources">
          {CREDIT_SOURCES.map((value) => (
            <FilterChip
              key={value}
              className="group"
              pressed={source === value}
              onPressedChange={(pressed) => setSource(pressed ? value : null)}
              data-source={value}
            >
              {creditSourceLabel[value]()}
              <CreditCountBadge count={counts[value]} />
            </FilterChip>
          ))}
        </div>
      </div>

      <SettingsCard data-slot="credits-card">
        <SettingsCardContent className="p-2">
          {packages.length > 0 ? (
            <CreditList packages={packages} onSelect={handleSelect} />
          ) : (
            <p className="text-on-surface-variant px-4 py-6 text-center text-sm">
              {m.settings_about_credits_no_match()}
            </p>
          )}
        </SettingsCardContent>
      </SettingsCard>

      <CreditLicenseModal
        pkg={selected}
        texts={credits.texts}
        open={modalOpen}
        onOpenChange={setModalOpen}
      />
    </>
  )
}

function RouteComponent() {
  const { data, error, isPending } = useOpenSourceCredits()

  return (
    <>
      <PageTitle backTo="/main/settings/about">
        {m.settings_about_credits_title()}
      </PageTitle>

      <div
        className="flex flex-col gap-4 px-4 pb-4"
        data-slot="credits-content"
      >
        <p className="text-on-surface-variant px-2 text-sm">
          {m.settings_about_credits_description()}
        </p>

        {isPending ? (
          <div
            className="text-on-surface-variant flex items-center gap-2 px-2 text-sm"
            data-slot="credits-loading"
          >
            <CircularProgress className="size-4" indeterminate />
            {m.settings_about_credits_loading()}
          </div>
        ) : error ? (
          <p className="text-error px-2 text-sm" role="alert">
            {m.settings_about_credits_failed({ error: formatError(error) })}
          </p>
        ) : data ? (
          <CreditsBrowser credits={data} />
        ) : (
          <SettingsCard data-slot="credits-missing">
            <SettingsCardContent>
              <p className="text-on-surface-variant text-sm">
                {m.settings_about_credits_missing()}
              </p>
            </SettingsCardContent>
          </SettingsCard>
        )}
      </div>
    </>
  )
}
