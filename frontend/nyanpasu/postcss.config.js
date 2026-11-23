import { fileURLToPath } from 'node:url'
import tailwindcss from '@tailwindcss/postcss'
import splitWhereCombinators from './postcss/split-where-combinators.js'

export default {
  plugins: [
    tailwindcss({ base: fileURLToPath(new URL('.', import.meta.url)) }),
    splitWhereCombinators(),
  ],
}
