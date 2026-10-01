import { z } from 'zod'

export const subscriptionUrlSchema = z.url({ protocol: /^https?$/ })
