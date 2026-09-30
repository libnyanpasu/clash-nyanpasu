import tailwindcss from '@tailwindcss/postcss'
import splitWhereCombinators from './postcss/split-where-combinators.js'

export default {
  plugins: [tailwindcss(), splitWhereCombinators()],
}
