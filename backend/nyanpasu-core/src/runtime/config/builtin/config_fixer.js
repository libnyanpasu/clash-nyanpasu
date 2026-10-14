export default function main(params) {
  if (typeof params['log-level'] === 'boolean') {
    params['log-level'] = 'debug'
  }
  return params
}
