import { useLanguage } from '@/components/providers/language-provider'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { m } from '@/paraglide/messages'
import { Locale, locales } from '@/paraglide/runtime'

export default function LanguageSelector() {
  const { language, setLanguage } = useLanguage()

  const handleLanguageChange = (value: string) => {
    setLanguage(value as Locale)
  }

  return (
    <Select
      variant="outlined"
      value={language}
      onValueChange={handleLanguageChange}
    >
      <SelectTrigger>
        <SelectValue placeholder={m.settings_user_interface_language_label()}>
          {language ? m.language(language, { locale: language }) : null}
        </SelectValue>
      </SelectTrigger>

      <SelectContent>
        {Object.entries(locales).map(([key, value]) => (
          <SelectItem key={key} value={value}>
            {m.language(key, { locale: value })}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
