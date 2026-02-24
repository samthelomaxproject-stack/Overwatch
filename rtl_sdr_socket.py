#!/usr/bin/env python3
"""RTL-SDR Bridge using TCP SOCKET (no pipe limits!)"""

import socket
import json
import sys
import time
import threading
from datetime import datetime

# TCP server settings (like Intercept)
TCP_HOST = "127.0.0.1"
TCP_PORT = 30004  # Different from dump1090's 30003
LOG_FILE = "/tmp/adsb_socket.log"

def log(msg):
    timestamp = datetime.now().strftime("%H:%M:%S.%f")[:-3]
    line = f"[{timestamp}] {msg}"
    with open(LOG_FILE, "a") as f:
        f.write(line + "\n")
    print(line, file=sys.stderr, flush=True)

class AircraftServer:
    """TCP server that clients (Rust) connect to"""
    def __init__(self):
        self.aircraft_db = {}
        self.clients = []
        self.lock = threading.Lock()
    
    def update_aircraft(self, icao, data):
        with self.lock:
            if icao not in self.aircraft_db:
                self.aircraft_db[icao] = {}
            self.aircraft_db[icao].update(data)
            self.aircraft_db[icao]['last_seen'] = time.time()
    
    def broadcast(self, message):
        """Send to all connected clients (must NOT hold self.lock when called)"""
        with self.lock:
            clients_snapshot = list(self.clients)
        
        dead_clients = []
        for client in clients_snapshot:
            try:
                client.send((message + "\n").encode('utf-8'))
            except:
                dead_clients.append(client)
        
        if dead_clients:
            with self.lock:
                for client in dead_clients:
                    if client in self.clients:
                        self.clients.remove(client)
    
    def run_server(self):
        """TCP server thread"""
        server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server.bind((TCP_HOST, TCP_PORT))
        server.listen(5)
        
        log(f"TCP server listening on {TCP_HOST}:{TCP_PORT}")
        
        while True:
            try:
                client, addr = server.accept()
                log(f"Client connected: {addr}")
                with self.lock:
                    self.clients.append(client)
            except Exception as e:
                log(f"Server error: {e}")
    
    def flush_to_clients(self):
        """Send all aircraft to clients every 500ms"""
        while True:
            time.sleep(0.5)
            
            # Build snapshot under lock, then broadcast without holding lock
            with self.lock:
                now = time.time()
                stale = [icao for icao, data in self.aircraft_db.items() 
                        if now - data.get('last_seen', 0) > 60]
                for icao in stale:
                    del self.aircraft_db[icao]
                
                messages = [
                    json.dumps({"aircraft": {k: v for k, v in data.items() if k != 'last_seen'}})
                    for data in self.aircraft_db.values()
                ]
            
            # Broadcast outside the lock
            for msg in messages:
                self.broadcast(msg)

def read_dump1090(server):
    """Read from dump1090 and update database"""
    while True:
        try:
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.settimeout(5)
            log(f"Connecting to dump1090 on 127.0.0.1:30003...")
            sock.connect(("127.0.0.1", 30003))
            log("✓ Connected to dump1090")
            
            buffer = ""
            
            while True:
                try:
                    data = sock.recv(4096).decode('utf-8', errors='ignore')
                    if not data:
                        log("dump1090 disconnected")
                        break
                    
                    buffer += data
                    
                    while '\n' in buffer:
                        line, buffer = buffer.split('\n', 1)
                        line = line.strip()
                        
                        if line.startswith('MSG,'):
                            parts = line.split(',')
                            if len(parts) >= 5:
                                icao = parts[4].upper()
                                msg_type = parts[1]
                                
                                aircraft_data = {"icao": icao, "msg_type": msg_type}
                                
                                if msg_type == '1' and len(parts) > 10:
                                    callsign = parts[10].strip()
                                    if callsign:
                                        aircraft_data["callsign"] = callsign
                                elif msg_type == '3' and len(parts) > 15:
                                    if parts[11]:
                                        try:
                                            aircraft_data["altitude"] = int(float(parts[11]))
                                        except:
                                            pass
                                    if parts[14] and parts[15]:
                                        try:
                                            aircraft_data["latitude"] = float(parts[14])
                                            aircraft_data["longitude"] = float(parts[15])
                                        except:
                                            pass
                                elif msg_type == '4' and len(parts) > 16:
                                    if parts[12]:
                                        try:
                                            aircraft_data["speed"] = int(float(parts[12]))
                                        except:
                                            pass
                                    if parts[13]:
                                        try:
                                            aircraft_data["heading"] = int(float(parts[13]))
                                        except:
                                            pass
                                
                                server.update_aircraft(icao, aircraft_data)
                                
                except socket.timeout:
                    continue
                except Exception as e:
                    log(f"Read error: {e}")
                    break
                    
        except Exception as e:
            log(f"Connection failed: {e}")
            time.sleep(5)

def main():
    open(LOG_FILE, "w").close()
    log("=" * 60)
    log("PYTHON TCP BRIDGE STARTED")
    log("=" * 60)
    
    server = AircraftServer()
    
    # Start TCP server thread
    server_thread = threading.Thread(target=server.run_server, daemon=True)
    server_thread.start()
    
    # Start broadcast thread
    broadcast_thread = threading.Thread(target=server.flush_to_clients, daemon=True)
    broadcast_thread.start()
    
    # Read from dump1090 (main thread)
    read_dump1090(server)

if __name__ == "__main__":
    main()