import { useTheme } from '@mui/material'
import { Clash } from '@nyanpasu/interface'

interface Props {
  index: number
  value: Clash.Rule
}

const RuleItem = ({ index, value }: Props) => {
  const { palette } = useTheme()

  const COLOR = [
    palette.primary.main,
    palette.secondary.main,
    palette.info.main,
    palette.warning.main,
    palette.success.main,
  ]

  const parseColor = (text: string) => {
    const TYPE = {
      reject: ['REJECT', 'REJECT-DROP'],
      direct: ['DIRECT'],
    }

    if (TYPE.reject.includes(text)) return palette.error.main

    if (TYPE.direct.includes(text)) return palette.text.primary

    let sum = 0

    for (let i = 0; i < text.length; i++) {
      sum += text.charCodeAt(i)
    }

    return COLOR[sum % COLOR.length]
  }

  return (
    <div className="flex p-2 pr-7 pl-7 select-text">
      <div style={{ color: palette.text.secondary }} className="min-w-14">
        {index + 1}
      </div>

      <div className="flex flex-col gap-1">
        <div style={{ color: palette.text.primary }}>
          {value.payload || '-'}
        </div>

        <div className="flex gap-8">
          <div className="min-w-40 text-sm">{value.type}</div>

          <div
            className="text-s text-sm"
            style={{ color: parseColor(value.proxy) }}
          >
            {value.proxy}
          </div>
        </div>
      </div>
    </div>
  )
}

export default RuleItem
