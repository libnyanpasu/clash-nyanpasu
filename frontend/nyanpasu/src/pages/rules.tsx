import { useDebounceEffect } from 'ahooks'
import { useSetAtom } from 'jotai'
import { lazy, RefObject, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { atomRulePage } from '@/components/rules/modules/store'
import { alpha, FilledInputProps, TextField, useTheme } from '@mui/material'
import { useClashCore } from '@nyanpasu/interface'
import { BasePage } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/rules')({
  component: RulesPage,
})

function RulesPage() {
  const { t } = useTranslation()

  const { palette } = useTheme()

  const { getRules } = useClashCore()

  const [filterText, setFilterText] = useState('')

  const setRule = useSetAtom(atomRulePage)

  const viewportRef = useRef<HTMLDivElement>(null)

  useDebounceEffect(
    () => {
      setRule({
        data: getRules.data?.rules.filter((each) =>
          each.payload.includes(filterText),
        ),
        scrollRef: viewportRef as RefObject<HTMLElement>,
      })
    },
    [getRules.data, viewportRef.current, filterText],
    { wait: 150 },
  )

  const inputProps: Partial<FilledInputProps> = {
    sx: {
      borderRadius: 7,
      backgroundColor: alpha(palette.primary.main, 0.1),

      fieldset: {
        border: 'none',
      },
    },
  }

  const Component = lazy(() => import('@/components/rules/rule-page'))

  return (
    <BasePage
      full
      title={t('Rules')}
      header={
        <TextField
          hiddenLabel
          autoComplete="off"
          spellCheck="false"
          value={filterText}
          placeholder={t('Filter conditions')}
          onChange={(e) => setFilterText(e.target.value)}
          className="!pb-0"
          sx={{ input: { py: 1, fontSize: 14 } }}
          InputProps={inputProps}
        />
      }
      viewportRef={viewportRef}
    >
      <Component />
    </BasePage>
  )
}
