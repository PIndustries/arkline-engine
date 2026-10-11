#!/usr/bin/env python3
"""Exercise the real server TLS listener with an ephemeral test CA.
Run after cargo build -p arkline-server: python3 tools/tests/test_origin_tls.py
Needs Python 3, openssl, and a local build; never contacts a live universe.
"""
import contextlib
import json
import os
from pathlib import Path
import socket
import shutil
import ssl
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
BINARY = Path(os.environ.get("ARKLINE_SERVER_BINARY", ROOT / "target/debug/arkline-server"))


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class OriginTLS(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.directory = Path(cls.tmp.name)
        cls.cert = cls.directory / "cert.pem"
        cls.key = cls.directory / "key.pem"
        cls.ca = cls.directory / "ca.pem"
        def openssl(*args):
            subprocess.run(["openssl", *map(str, args)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                "-keyout", cls.directory / "ca.key", "-out", cls.ca, "-subj", "/CN=Arkline Test CA",
                "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign")
        openssl("req", "-new", "-newkey", "rsa:2048", "-nodes", "-keyout", cls.key,
                "-out", cls.directory / "leaf.csr", "-subj", "/CN=localhost")
        (cls.directory / "leaf.ext").write_text("subjectAltName=DNS:localhost\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
        openssl("x509", "-req", "-in", cls.directory / "leaf.csr", "-CA", cls.ca, "-CAkey", cls.directory / "ca.key",
                "-CAcreateserial", "-days", "1", "-extfile", cls.directory / "leaf.ext", "-out", cls.cert)
        cls.projection = cls.directory / "projected"
        cls.projection.mkdir()
        first = cls.projection / "generation-1"
        first.mkdir()
        shutil.copy(cls.cert, first / "cert.pem")
        shutil.copy(cls.key, first / "key.pem")
        (cls.projection / "..data").symlink_to("generation-1")
        (cls.projection / "cert.pem").symlink_to("..data/cert.pem")
        (cls.projection / "key.pem").symlink_to("..data/key.pem")
        cls.cert, cls.key = cls.projection / "cert.pem", cls.projection / "key.pem"
        cls.raw, cls.tls = port(), port()
        cls.process = subprocess.Popen([str(BINARY), "--bind", f"127.0.0.1:{cls.raw}",
            "--ws-bind", f"127.0.0.1:{cls.tls}", "--tls-cert", str(cls.cert), "--tls-key", str(cls.key),
            "--db", str(cls.directory / "world.sqlite")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(100):
            try:
                with socket.create_connection(("127.0.0.1", cls.raw), timeout=.1):
                    break
            except OSError:
                if cls.process.poll() is not None:
                    raise RuntimeError("server failed to start")
                time.sleep(.05)
        time.sleep(.2)

    @classmethod
    def tearDownClass(cls):
        cls.process.terminate()
        cls.process.wait(timeout=5)
        cls.tmp.cleanup()

    def connect(self, hostname="localhost", trust=True):
        ctx = ssl.create_default_context(cafile=str(self.ca) if trust else None)
        sock = socket.create_connection(("127.0.0.1", self.tls), timeout=2)
        try:
            return ctx.wrap_socket(sock, server_hostname=hostname)
        except BaseException:
            sock.close()
            raise

    def test_projected_rotation_and_invalid_pair_fail_closed(self):
        generation = self.projection / "generation-2"
        generation.mkdir()
        shutil.copy(self.key, generation / "key.pem")
        subprocess.run(["openssl", "x509", "-req", "-in", str(self.directory / "leaf.csr"), "-CA", str(self.ca),
            "-CAkey", str(self.directory / "ca.key"), "-set_serial", "424242", "-days", "1", "-extfile", str(self.directory / "leaf.ext"), "-out", str(generation / "cert.pem")], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        def switch(name):
            pending = self.projection / "..data-next"
            pending.symlink_to(name)
            pending.replace(self.projection / "..data")
        try:
            with self.connect() as existing:
                old = existing.getpeercert(binary_form=True)
                switch("generation-2")
                with self.connect() as renewed:
                    self.assertNotEqual(old, renewed.getpeercert(binary_form=True))
                bad = self.projection / "generation-bad"
                bad.mkdir()
                shutil.copy(generation / "cert.pem", bad / "cert.pem")
                shutil.copy(self.directory / "ca.key", bad / "key.pem")
                switch("generation-bad")
                with self.assertRaises((ssl.SSLError, ConnectionResetError)):
                    self.connect()
                # An already established session remains usable during rotation.
                existing.sendall(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
                reply = b""
                while chunk := existing.recv(4096):
                    reply += chunk
                self.assertIn(b"200 OK", reply)
        finally:
            switch("generation-1")
        with self.connect() as recovered:
            self.assertEqual(old, recovered.getpeercert(binary_form=True))
        self.assertIsNone(self.process.poll())

    def test_verified_readiness_command(self):
        result = subprocess.run([str(BINARY), "--tls-health", f"127.0.0.1:{self.tls}", "--tls-ca", str(self.ca), "--tls-server-name", "localhost"], capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        result = subprocess.run([str(BINARY), "--tls-health", f"127.0.0.1:{self.tls}", "--tls-ca", str(self.ca), "--tls-server-name", "wrong.invalid"], capture_output=True)
        self.assertNotEqual(result.returncode, 0)

    def test_stalled_tls_handshake_is_bounded(self):
        with socket.create_connection(("127.0.0.1", self.tls), timeout=7) as s:
            started = time.monotonic()
            self.assertEqual(s.recv(1), b"")
            self.assertLess(time.monotonic() - started, 6.5)

    def test_verified_tls_health(self):
        with self.connect() as s:
            s.sendall(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            reply = b""
            while chunk := s.recv(4096):
                reply += chunk
            self.assertIn(b"200 OK", reply)
            self.assertIn(b'"simulation_ready":true', reply)

    def test_untrusted_certificate_rejected(self):
        with self.assertRaises(ssl.SSLCertVerificationError):
            self.connect(trust=False)

    def test_wrong_server_identity_rejected(self):
        with self.assertRaises(ssl.SSLCertVerificationError):
            self.connect(hostname="wrong.invalid")

    def test_plaintext_not_accepted_on_tls_port(self):
        with socket.create_connection(("127.0.0.1", self.tls), timeout=2) as s:
            s.sendall(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            with contextlib.suppress(ConnectionResetError):
                self.assertNotIn(b"200 OK", s.recv(4096))

    def test_websocket_origin_rejected_over_tls(self):
        with self.connect() as s:
            s.sendall(b"GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nOrigin: https://untrusted.invalid\r\n\r\n")
            self.assertIn(b"403", s.recv(4096))

    def test_mismatched_key_fails_before_opening_world(self):
        db = self.directory / "mismatch-must-not-exist.sqlite"
        result = subprocess.run([str(BINARY), "--ws-bind", "127.0.0.1:0", "--tls-cert", str(self.cert), "--tls-key", str(self.directory / "ca.key"), "--db", str(db)], capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(db.exists())

    def test_missing_key_fails_before_opening_world(self):
        db = self.directory / "must-not-exist.sqlite"
        result = subprocess.run([str(BINARY), "--ws-bind", "127.0.0.1:0", "--tls-cert", str(self.cert), "--db", str(db)], capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(db.exists())

    def test_real_websocket_message_over_verified_tls(self):
        with self.connect() as s:
            s.sendall(b"GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nOrigin: https://play.arkline.network\r\n\r\n")
            header = b""
            while b"\r\n\r\n" not in header:
                header += s.recv(1)
            self.assertIn(b"101 Switching Protocols", header)
            # A masked invalid-schema request must produce a real application reply.
            payload, mask = b'{}', os.urandom(4)
            s.sendall(bytes([0x81, 0x80 | len(payload)]) + mask + bytes(v ^ mask[i % 4] for i, v in enumerate(payload)))
            head = s.recv(2)
            length = head[1] & 127
            if length == 126:
                length = int.from_bytes(s.recv(2), "big")
            body = b""
            while len(body) < length:
                body += s.recv(length - len(body))
            self.assertFalse(json.loads(body)["ok"])


if __name__ == "__main__":
    unittest.main()
