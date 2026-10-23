const http = require('node:http')
const port = Number(process.env.PORT ?? 8080)
const server = http.createServer((req, res) => {
  if (req.url === '/health') return res.end('ok')
  res.setHeader('content-type', 'text/html; charset=utf-8')
  res.end('<!doctype html><title>hello from redone</title><body style="font:20px system-ui;margin:48px 16px">Hello from <b>redone</b>: planned by an agent, approved by a person, running on loam.</body>')
})
server.listen(port, '0.0.0.0', () => console.log(`hello listening on ${port}`))
process.on('SIGTERM', () => server.close(() => process.exit(0)))
