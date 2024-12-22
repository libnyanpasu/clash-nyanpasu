import { ChangeEvent, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import Done from '@mui/icons-material/Done'
import {
  Box,
  Button,
  Divider,
  TextField,
  TextFieldProps,
  Typography,
} from '@mui/material'
import { Expand } from '../expand'
import { BaseItem } from './baseItem'

export interface NumberItemProps {
  label: string
  value: number
  checkEvent: (input: number) => boolean
  checkLabel: string
  onApply: (input: number) => void
  divider?: boolean
  textFieldProps?: TextFieldProps
}

/**
 * @example
 * <NumberItem
    label={t("Mixed Port")}
    value={port}
    checkEvent={(input) => input > 65535 || input < 1}
    checkLabel="Port must be between 1 and 65535."
    onApply={(value) => {
      setConfigs({ "mixed-port": value });
    }}
    />
 *
 * @returns {React.JSX.Element}
 * React.JSX.Element
 *
 * `NumberItem most use for port label.`
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const NumberItem = ({
  label,
  value,
  checkEvent,
  checkLabel,
  onApply,
  divider,
  textFieldProps,
}: NumberItemProps) => {
  const { t } = useTranslation()

  const [changed, setChanged] = useState(false)

  const [input, setInput] = useState<number | null>(null)

  const applyCheck = useMemo(
    () => checkEvent(input as number),
    [checkEvent, input],
  )

  return (
    <>
      <BaseItem title={label}>
        <TextField
          value={input !== null ? input : value}
          size="small"
          variant="outlined"
          sx={{ width: 80 }}
          inputProps={{ 'aria-autocomplete': 'none' }}
          onChange={(e: ChangeEvent<HTMLInputElement>) => {
            setInput(Number(e.target.value))
            setChanged(true)
          }}
          {...textFieldProps}
        />
      </BaseItem>

      <Expand open={changed}>
        <Box
          sx={{ pb: 1 }}
          display="flex"
          justifyContent="space-between"
          alignItems="center"
        >
          <span>
            {applyCheck && (
              <Typography variant="body2" color="error">
                {checkLabel}
              </Typography>
            )}
          </span>

          <Button
            variant="contained"
            startIcon={<Done />}
            disabled={applyCheck}
            onClick={() => {
              onApply(input as number)
              setChanged(false)
            }}
          >
            {t('Apply')}
          </Button>
        </Box>

        {divider && <Divider />}
      </Expand>
    </>
  )
}
