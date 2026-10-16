"""Fresh PR2646 native transport characterization; NOT dot's missing packet.

Run against owner e254af1 with generated repository protobuf bindings:
PYTHONPATH=python python -m pytest -q review-evidence/2646-native-ipv6_test.py

Uses the complete SandboxClient.from_env -> gRPC channel -> generated Health
RPC path and a real loopback gRPC service, not an OpenShell gateway deployment.
No SDK source is replaced or monkeypatched. Only OPENSHELL_* environment
configuration is isolated. Temporary self-signed TLS material never leaves tmp.
"""
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta, timezone
import ipaddress
import os

import grpc
import pytest
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID

from openshell import SandboxClient
from openshell._proto import openshell_pb2, openshell_pb2_grpc


@pytest.fixture
def tls_material(tmp_path):
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "lane17-loopback")])
    now = datetime.now(timezone.utc)
    cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - timedelta(minutes=1))
        .not_valid_after(now + timedelta(hours=1))
        .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
        .add_extension(x509.SubjectAlternativeName([
            x509.IPAddress(ipaddress.ip_address("127.0.0.1")),
            x509.IPAddress(ipaddress.ip_address("::1")),
        ]), critical=False)
        .sign(key, hashes.SHA256())
    )
    cert_pem = cert.public_bytes(serialization.Encoding.PEM)
    key_pem = key.private_bytes(serialization.Encoding.PEM,
                                serialization.PrivateFormat.PKCS8,
                                serialization.NoEncryption())
    ca_path = tmp_path / "loopback-ca.pem"
    ca_path.write_bytes(cert_pem)
    return key_pem, cert_pem, ca_path


@pytest.mark.parametrize("tls", [False, True], ids=["plaintext", "tls"])
@pytest.mark.parametrize("host", ["127.0.0.1", "[::1]"], ids=["ipv4", "ipv6"])
@pytest.mark.parametrize("url", [False, True], ids=["bare", "url"])
def test_from_env_health_native(monkeypatch, tls_material, tls, host, url):
    received = []

    class Service(openshell_pb2_grpc.OpenShellServicer):
        def Health(self, request, context):
            received.append(dict(context.invocation_metadata()))
            return openshell_pb2.HealthResponse()

    for name in list(os.environ):
        if name.startswith("OPENSHELL_"):
            monkeypatch.delenv(name)
    key_pem, cert_pem, ca_path = tls_material
    with ThreadPoolExecutor(max_workers=2) as executor:
        server = grpc.server(executor)
        openshell_pb2_grpc.add_OpenShellServicer_to_server(Service(), server)
        if tls:
            port = server.add_secure_port(f"{host}:0", grpc.ssl_server_credentials(
                [(key_pem, cert_pem)]))
        else:
            port = server.add_insecure_port(f"{host}:0")
        assert port != 0, "native loopback binding unavailable"
        server.start()
        endpoint = f"{host}:{port}"
        if url:
            endpoint = ("https://" if tls else "http://") + endpoint
        monkeypatch.setenv("OPENSHELL_ENDPOINT", endpoint)
        monkeypatch.setenv("OPENSHELL_TOKEN", "lane17-loopback-only")
        monkeypatch.setenv("OPENSHELL_TIMEOUT", "1")
        if tls:
            monkeypatch.setenv("OPENSHELL_TLS", "true")
            monkeypatch.setenv("OPENSHELL_TLS_CA", str(ca_path))
        try:
            with SandboxClient.from_env() as client:
                response = client.health()
            assert isinstance(response, openshell_pb2.HealthResponse)
            assert received == [{"user-agent": received[0]["user-agent"],
                                 "authorization": "Bearer lane17-loopback-only"}]
        finally:
            server.stop(0).wait()
