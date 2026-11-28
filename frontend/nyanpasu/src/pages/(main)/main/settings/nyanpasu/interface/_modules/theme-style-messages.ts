import { m } from '@/paraglide/messages'
import type { ThemeVariant } from '@nyanpasu/theme'

/** The name and summary of each scheme style, in display order. */
export const THEME_STYLE_MESSAGES: readonly {
  variant: ThemeVariant
  label: () => string
  summary: () => string
}[] = [
  {
    variant: 'tonalSpot',
    label: m.settings_user_interface_theme_style_tonal_spot,
    summary: m.settings_user_interface_theme_style_tonal_spot_summary,
  },
  {
    variant: 'expressive',
    label: m.settings_user_interface_theme_style_expressive,
    summary: m.settings_user_interface_theme_style_expressive_summary,
  },
  {
    variant: 'vibrant',
    label: m.settings_user_interface_theme_style_vibrant,
    summary: m.settings_user_interface_theme_style_vibrant_summary,
  },
  {
    variant: 'fidelity',
    label: m.settings_user_interface_theme_style_fidelity,
    summary: m.settings_user_interface_theme_style_fidelity_summary,
  },
  {
    variant: 'content',
    label: m.settings_user_interface_theme_style_content,
    summary: m.settings_user_interface_theme_style_content_summary,
  },
  {
    variant: 'neutral',
    label: m.settings_user_interface_theme_style_neutral,
    summary: m.settings_user_interface_theme_style_neutral_summary,
  },
  {
    variant: 'monochrome',
    label: m.settings_user_interface_theme_style_monochrome,
    summary: m.settings_user_interface_theme_style_monochrome_summary,
  },
  {
    variant: 'rainbow',
    label: m.settings_user_interface_theme_style_rainbow,
    summary: m.settings_user_interface_theme_style_rainbow_summary,
  },
  {
    variant: 'fruitSalad',
    label: m.settings_user_interface_theme_style_fruit_salad,
    summary: m.settings_user_interface_theme_style_fruit_salad_summary,
  },
]
