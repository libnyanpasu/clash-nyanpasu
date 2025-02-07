import { Hono } from 'jsr:@hono/hono@4.7.0'
import releaseBeta from './release-beta.json' with { type: 'json' }
import releaseStable from './release-stable.json' with { type: 'json' }

const app = new Hono()

app.get('/', (c) => c.text('Clash Nyanpasu :3'))

app.get('/release-stable.json', (c) => {
  return c.json(releaseStable)
})

app.get('/release-beta.json', (c) => {
  return c.json(releaseBeta)
})

Deno.serve(app.fetch)
