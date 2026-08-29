import http.server
import json
import socketserver
import threading
import time
import uuid
from datetime import datetime, timezone

PORT = 8899
DRIFT_ENABLED = False

class ApiSnapMockHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass # Suppress standard log output

    def do_GET(self):
        global DRIFT_ENABLED
        if self.path.startswith("/api/v1/users/1"):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("X-Custom-Header", "ApiSnap-Mock-v1")
            self.end_headers()
            
            # Dynamic volatile fields (UUID, timestamp, JWT-like token, valid Luhn test card)
            data = {
                "user_id": str(uuid.uuid4()),
                "username": "alice_developer",
                "email": "alice@example.com",
                "created_at": datetime.now(timezone.utc).isoformat(),
                "session_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgN_p_placeholder",
                "credit_card": "4532-0151-1283-0366",
                "tier": "enterprise"
            }
            self.wfile.write(json.dumps(data).encode("utf-8"))

        elif self.path.startswith("/api/v1/products"):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            data = {
                "products": [
                    {"id": "PROD-101", "name": "Rust Systems Guide", "price": 49.99},
                    {"id": "PROD-102", "name": "Cloud Native Architecture", "price": 89.50}
                ]
            }
            self.wfile.write(json.dumps(data).encode("utf-8"))

        elif self.path.startswith("/api/v1/drift/test"):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            if DRIFT_ENABLED:
                # Simulated breaking regression
                data = {
                    "account_id": "ACC-999",
                    "status": "SUSPENDED", # DRIFT! (was ACTIVE)
                    "new_unannounced_field": "surprise"
                }
            else:
                data = {
                    "account_id": "ACC-999",
                    "status": "ACTIVE"
                }
            self.wfile.write(json.dumps(data).encode("utf-8"))

        elif self.path.startswith("/api/v1/toggle_drift"):
            DRIFT_ENABLED = not DRIFT_ENABLED
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"drift_enabled": DRIFT_ENABLED}).encode("utf-8"))

        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self):
        content_len = int(self.headers.get('Content-Length', 0))
        post_body = self.rfile.read(content_len) if content_len > 0 else b"{}"
        
        if self.path.startswith("/api/v1/orders"):
            self.send_response(201)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            data = {
                "order_id": f"ORD-{int(time.time())}",
                "status": "created",
                "timestamp": datetime.now(timezone.utc).isoformat()
            }
            self.wfile.write(json.dumps(data).encode("utf-8"))

        elif self.path.startswith("/api/v1/fuzz/target"):
            try:
                payload = json.loads(post_body.decode('utf-8'))
                if "crash" in str(payload):
                    self.send_response(500)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(b'{"error": "Simulated Internal Server Error"}')
                else:
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(b'{"status": "handled"}')
            except Exception as e:
                self.send_response(400)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({"error": f"Malformed payload: {str(e)}"}).encode("utf-8"))

        else:
            self.send_response(404)
            self.end_headers()

def run_server():
    server = socketserver.TCPServer(("127.0.0.1", PORT), ApiSnapMockHandler)
    server.allow_reuse_address = True
    server.serve_forever()

if __name__ == "__main__":
    print(f"Starting ApiSnap E2E Mock Server on http://127.0.0.1:{PORT}")
    run_server()
