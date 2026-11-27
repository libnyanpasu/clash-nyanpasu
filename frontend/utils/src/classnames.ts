import { clsx, type ClassValue } from 'clsx'
import { extendTailwindMerge } from 'tailwind-merge'

const twMerge = extendTailwindMerge({
  extend: {
    theme: {
      radius: [
        'extra-small',
        'small',
        'medium',
        'large',
        'large-increased',
        'extra-large',
        'extra-large-increased',
        'extra-extra-large',
        'lg-inc',
        'xl-inc',
        'exl',
      ],
    },
  },
})

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(...inputs))
}
