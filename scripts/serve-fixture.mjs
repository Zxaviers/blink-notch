// Local calendar server for manual and end-to-end checks.
//   node scripts/serve-fixture.mjs [port]
// Serves an .ics whose deadlines are relative to "now", with ETag support so
// the conditional-GET path (304) can be exercised too.

import http from 'node:http'
import crypto from 'node:crypto'

const port = Number(process.argv[2]) || 8765
const H = 3600 * 1000

const stamp = (ms) => new Date(ms).toISOString().replace(/[-:]/g, '').replace(/\.\d{3}/, '')

function buildIcs () {
  const now = Date.now()
  const events = [
    ['t1', 'Problem Set 4 is due', 'CS101 Algorithms', now + 3 * H + 14 * 60 * 1000],
    ['t2', 'Quiz 3 closes', 'MA201 Linear Algebra', now + 30 * H],
    ['t3', 'Lab report is due', 'PH110 Physics Lab', now + 4 * 24 * H],
    ['t4', 'Essay draft is due', 'EN150 Writing', now - 5 * H],
    ['t5', 'Project proposal is due', 'Basis Data', now + 11 * 24 * H]
  ]
  const lines = ['BEGIN:VCALENDAR', 'VERSION:2.0', 'PRODID:-//Deadline Notch fixture//EN']
  for (const [uid, summary, course, due] of events) {
    lines.push(
      'BEGIN:VEVENT',
      `UID:${uid}@fixture.local`,
      `SUMMARY:${summary}`,
      `DTSTART:${stamp(due)}`,
      `CATEGORIES:${course}`,
      `URL:https://example.edu/mod/assign/view.php?id=${uid}`,
      'END:VEVENT'
    )
  }
  lines.push('END:VCALENDAR')
  return `${lines.join('\r\n')}\r\n`
}

let requests = 0
http.createServer((req, res) => {
  requests += 1
  const body = buildIcs()
  // Stable per hour so "not modified" is reachable.
  const etag = `"${crypto.createHash('md5').update(body.replace(/DTSTART:\d{8}T\d{6}Z/g, '')).digest('hex')}"`
  if (req.headers['if-none-match'] === etag) {
    res.writeHead(304, { ETag: etag })
    res.end()
    console.log(`#${requests} ${req.method} -> 304`)
    return
  }
  res.writeHead(200, { 'Content-Type': 'text/calendar; charset=utf-8', ETag: etag })
  res.end(body)
  console.log(`#${requests} ${req.method} -> 200`)
}).listen(port, '127.0.0.1', () => console.log(`fixture calendar on http://127.0.0.1:${port}/cal.ics`))
