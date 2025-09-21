const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB']

const parseTraffic = (num?: string | number) => {
  if (typeof num !== 'number') {
    const tmp = Number(num)
    if (isNaN(tmp)) return ['NaN', '']
    num = tmp
  }

  // 处理负数或零的情况
  if (num <= 0) return ['0', 'B']

  // 使用 Math.log 而不是 Math.log2 来提高精度
  const exp = Math.min(
    Math.floor(Math.log(num) / Math.log(1024)),
    UNITS.length - 1,
  )
  const dat = num / Math.pow(1024, exp)

  // 对于非常小的数字，确保至少显示一位小数
  let ret: string
  if (dat < 1) {
    ret = dat.toPrecision(2)
  } else if (dat < 10) {
    ret = dat.toPrecision(3)
  } else {
    ret = dat >= 1000 ? dat.toFixed(0) : dat.toPrecision(3)
  }

  const unit = UNITS[exp]

  return [ret, unit]
}

export default parseTraffic
