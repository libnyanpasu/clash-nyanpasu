import { TelegramClient } from 'telegram'
import { StringSession } from 'telegram/sessions'

if (!process.env.TELEGRAM_API_ID) {
  throw new Error('TELEGRAM_API_ID is required')
}

const TELEGRAM_API_ID = Number(process.env.TELEGRAM_API_ID)

if (!process.env.TELEGRAM_API_HASH) {
  throw new Error('TELEGRAM_API_ID is required')
}

const TELEGRAM_API_HASH = process.env.TELEGRAM_API_HASH

export const client = new TelegramClient(
  new StringSession(''),
  TELEGRAM_API_ID,
  TELEGRAM_API_HASH,
  {
    connectionRetries: 5,
  },
)
