import { createTiming } from './utils'

export const timing = {
  Google: createTiming('https://www.gstatic.com/generate_204'),
  GitHub: createTiming('https://github.com/', 200),
  BingCN: createTiming('https://cn.bing.com/', 200),
  Baidu: createTiming('https://www.baidu.com/', 200),
}
