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
  // dedent ignores unindented lines when computing the common margin.
  // A zero-indent YAML root must keep its children's indentation.
  return YAML.parse(/^\S/m.test(str) ? str : dedent(str))
}
