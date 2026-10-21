"""A stand-in for Arugula control: enough for the app's sign-in to start.

POST /auth/app answers with a ticket, as control's app_login.rs does; every
request goes to the log. Writes the port it took to argv[1].

With `--pages DIR` (platform.sh, M157) it's also control's page and the
platform page's, on every loopback address (127.0.0.1 is control,
127.0.0.2 the platform page, 127.0.0.3 a lookalike), and on a second port
too (DIR/other-port), another lookalike:

  GET /control.json  DIR/control.json, what control says of itself
  GET /cmd?path=P    the next command for the page at this host and path
                     (DIR/cmd/<host>:<port><path, / as _>, taken once read)
  GET /report?...    what a page says: logged as `report <host> k=v ...`
  GET anything else  the page (page.html beside this file): it reports
                     `what=loaded`, then runs the commands it's given

Each request's line in the log says the Host it was made to.
"""

import json
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qsl, urlsplit

port_file, log = sys.argv[1], sys.argv[2]
pages = sys.argv[4] if len(sys.argv) > 4 and sys.argv[3] == "--pages" else None
page_html = os.path.join(os.path.dirname(os.path.abspath(__file__)), "page.html")
lock = threading.Lock()


def write(line):
    with lock, open(log, "a") as f:
        f.write(line + "\n")
        f.flush()


class Control(BaseHTTPRequestHandler):
    def note(self, body=b""):
        write(f"{self.command} {self.path} host={self.headers.get('host', '')} {body.decode(errors='replace')}")

    def send(self, status, body, kind):
        self.send_response(status)
        self.send_header("content-type", kind)
        self.send_header("content-length", str(len(body)))
        self.send_header("cache-control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def answer(self, status, body):
        self.send(status, json.dumps(body).encode(), "application/json")

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("content-length") or 0))
        self.note(body)
        if self.path == "/auth/app":
            base = f"http://127.0.0.1:{self.server.server_port}"
            self.answer(200, {"ticket": "t1", "code": "WXYZ-1234", "url": f"{base}/#app=t1"})
        else:
            self.answer(404, {"error": "not here"})

    def do_GET(self):
        url = urlsplit(self.path)
        query = dict(parse_qsl(url.query))
        host = self.headers.get("host", "")
        if pages is None:
            self.note()
            self.answer(404, {"error": "not here"})
        elif url.path == "/cmd":
            # Polled: not logged.
            f = os.path.join(pages, "cmd", host + query.get("path", "/").replace("/", "_"))
            try:
                with lock:
                    cmd = open(f).read()
                    os.remove(f)
            except FileNotFoundError:
                self.send(204, b"", "text/plain")
                return
            write(f"cmd {host} {cmd.strip()}")
            self.send(200, cmd.encode(), "text/plain")
        elif url.path == "/report":
            write(f"report {host} " + " ".join(f"{k}={v}" for k, v in query.items()))
            self.send(204, b"", "text/plain")
        elif url.path == "/control.json":
            self.note()
            try:
                body = open(os.path.join(pages, "control.json"), "rb").read()
            except FileNotFoundError:
                body = b"{}"
            self.send(200, body, "application/json")
        else:
            self.note()
            self.send(200, open(page_html, "rb").read(), "text/html; charset=utf-8")

    def log_message(self, *args):
        pass


if pages is None:
    srv = ThreadingHTTPServer(("127.0.0.1", 0), Control)
else:
    # Every loopback address: 127.0.0.2 and .3 are other hosts on this port.
    srv = ThreadingHTTPServer(("0.0.0.0", 0), Control)
    other = ThreadingHTTPServer(("0.0.0.0", 0), Control)
    threading.Thread(target=other.serve_forever, daemon=True).start()
    with open(os.path.join(pages, "other-port.tmp"), "w") as f:
        f.write(str(other.server_port))
    os.rename(os.path.join(pages, "other-port.tmp"), os.path.join(pages, "other-port"))
with open(port_file + ".tmp", "w") as f:
    f.write(str(srv.server_port))
os.rename(port_file + ".tmp", port_file)
srv.serve_forever()
