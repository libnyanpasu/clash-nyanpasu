import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import CLASH_FIELD from '@/assets/json/clash-field.json'
import { Box, Typography } from '@mui/material'
import Grid from '@mui/material/Grid2'
import { useClash, useNyanpasu, useProfile } from '@nyanpasu/interface'
import { BaseCard, BaseDialog } from '@nyanpasu/ui'
import { ClashFieldItem, LabelSwitch } from './modules/clash-field'

const FieldsControl = ({
  label,
  fields,
  enabledFields,
  onChange,
}: {
  label: string
  fields: { [key: string]: string }
  enabledFields?: string[]
  onChange?: (key: string) => void
}) => {
  const [open, setOpen] = useState(false)

  // Nyanpasu Control Fields object key
  const disabled = label === 'default' || label === 'handle'

  const showFields: string[] = disabled
    ? Object.entries(fields).map(([key]) => key)
    : (enabledFields as string[])

  const Item = () => {
    return Object.entries(fields).map(([fKey, fValue], fIndex) => {
      const checked = enabledFields?.includes(fKey)

      return (
        <LabelSwitch
          key={fIndex}
          label={fKey}
          url={fValue}
          disabled={disabled}
          checked={disabled ? true : checked}
          onChange={onChange ? () => onChange(fKey) : undefined}
        />
      )
    })
  }

  return (
    <>
      <ClashFieldItem
        label={label}
        fields={showFields}
        onClick={() => setOpen(true)}
      />

      <BaseDialog
        title={label}
        open={open}
        close="Close"
        onClose={() => setOpen(false)}
        divider
        contentStyle={{ overflow: 'auto' }}
      >
        <Box display="flex" flexDirection="column" gap={1}>
          {disabled && <Typography>Clash Nyanpasu Control Fields.</Typography>}

          <Item />
        </Box>
      </BaseDialog>
    </>
  )
}

const ClashFieldSwitch = () => {
  const { t } = useTranslation()

  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()

  return (
    <LabelSwitch
      label={t('Enable Clash Fields Filter')}
      checked={nyanpasuConfig?.enable_clash_fields}
      onChange={() =>
        setNyanpasuConfig({
          enable_clash_fields: !nyanpasuConfig?.enable_clash_fields,
        })
      }
    />
  )
}

export const SettingClashField = () => {
  const { t } = useTranslation()

  const { query, upsert } = useProfile()

  const mergeFields = useMemo(
    () => [
      ...[
        ...Object.keys(CLASH_FIELD.default),
        ...Object.keys(CLASH_FIELD.handle),
      ],
      ...(query.data?.valid ?? []),
    ],
    [query.data],
  )

  const filteredField = (fields: { [key: string]: string }): string[] => {
    const usedObjects = []

    for (const key in fields) {
      if (
        Object.prototype.hasOwnProperty.call(fields, key) &&
        mergeFields.includes(key)
      ) {
        usedObjects.push(key)
      }
    }

    return usedObjects
  }

  const updateFiled = async (key: string) => {
    const getFields = (): string[] => {
      const valid = query.data?.valid ?? []

      if (valid.includes(key)) {
        return valid.filter((item) => item !== key)
      } else {
        valid.push(key)

        return valid
      }
    }

    await upsert.mutateAsync({ valid: getFields() })
  }

  return (
    <BaseCard label={t('Clash Field')}>
      <Box sx={{ pt: 1, pb: 2 }}>
        <ClashFieldSwitch />
      </Box>

      <Grid container spacing={2}>
        {Object.entries(CLASH_FIELD).map(([key, value], index) => {
          const filtered = filteredField(value)

          return (
            <FieldsControl
              key={index}
              label={key}
              fields={value}
              enabledFields={filtered}
              onChange={updateFiled}
            />
          )
        })}
      </Grid>
    </BaseCard>
  )
}

export default SettingClashField
