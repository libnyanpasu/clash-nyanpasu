import dedent from 'nyan:dedent'
import YAML from 'nyan:yaml'

/**
 * Parse template string into YAML object
 * @param {TemplateStringsArray} strings Template string array
 * @param {...any} values Template string interpolation values
 * @returns {Object} Parsed YAML object
 */
export function yaml(strings, ...values) {
  const str = String.raw({ raw: strings }, ...values)
  return YAML.parse(dedent(str))
}
