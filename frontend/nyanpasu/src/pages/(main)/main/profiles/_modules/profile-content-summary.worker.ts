import { profileContentSummary } from './profile-content-summary'

// Parses profile files off the main thread: a large subscription takes
// hundreds of milliseconds to parse.
self.onmessage = ({ data }: MessageEvent<{ id: number; content: string }>) => {
  self.postMessage({
    id: data.id,
    summary: profileContentSummary(data.content),
  })
}
