import figlet from 'figlet'
import { filesize } from 'filesize'
import fs from 'fs-extra'
import { HttpsProxyAgent } from 'https-proxy-agent'
import { GITHUB_PROXY } from './env'

export const getGithubUrl = (url: string) => {
  return new URL(url.replace(/^https?:\/\//g, ''), GITHUB_PROXY).toString()
}

export const getFileSize = (path: string): string => {
  const stat = fs.statSync(path)
  return filesize(stat.size)
}

export const array2text = (
  array: string[],
  type: 'newline' | 'space' = 'newline',
): string => {
  let result = ''

  const getSplit = () => {
    if (type === 'newline') {
      return '\n'
    } else if (type === 'space') {
      return ' '
    }
  }

  array.forEach((value, index) => {
    if (index === array.length - 1) {
      result += value
    } else {
      result += value + getSplit()
    }
  })

  return result
}

export const printNyanpasu = () => {
  const ascii = figlet.textSync('Clash Nyanpasu', {
    whitespaceBreak: true,
  })

  console.log(ascii)
}

export const HTTP_PROXY =
  process.env.HTTP_PROXY ||
  process.env.http_proxy ||
  process.env.HTTPS_PROXY ||
  process.env.https_proxy

export function getProxyAgent() {
  if (HTTP_PROXY) {
    return new HttpsProxyAgent(HTTP_PROXY)
  }

  return undefined
}
