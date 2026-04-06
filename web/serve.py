#!/usr/bin/env python3
"""Simple HTTP server with correct MIME types for WASM."""
import http.server
import os

os.chdir(os.path.dirname(os.path.abspath(__file__)))

handler = http.server.SimpleHTTPRequestHandler
handler.extensions_map.update({
    '.wasm': 'application/wasm',
    '.js': 'application/javascript',
    '.mjs': 'application/javascript',
})

server = http.server.HTTPServer(('', 8090), handler)
print("Serving at http://localhost:8090")
server.serve_forever()
