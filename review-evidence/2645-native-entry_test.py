"""Fresh native PR2645 entry lifecycle proof, not the missing historical packet.

Uses real SDK-owned grpc.aio channels and generated RPCs with a real loopback
service. No fake client, stub/channel substitution, or extracted entry helper.
The test observes private wrapper state but never replaces production objects.
Failed-head retained channels are explicitly closed by test cleanup afterwards;
this characterizes missing deterministic cleanup, not a permanent process leak.
"""
import asyncio
import json

import grpc
import pytest

from openshell.sandbox import AsyncSandbox
from openshell._proto import openshell_pb2, openshell_pb2_grpc


@pytest.mark.parametrize("stage", ["create", "get", "ready"])
@pytest.mark.parametrize("failure", ["rpc", "cancel"])
async def test_failed_entry_closes_owned_channel(tmp_path, monkeypatch, stage, failure):
    await exercise(tmp_path, monkeypatch, stage, failure)


@pytest.mark.parametrize("stage", ["create", "get"])
async def test_successful_entry_preserves_channel_until_exit(tmp_path, monkeypatch, stage):
    await exercise(tmp_path, monkeypatch, stage, "success")


async def exercise(tmp_path, monkeypatch, stage, failure):
    blocked = asyncio.Event()
    channels = []
    requests = []
    ctx = AsyncSandbox(workspace="default", cluster="native-entry",
                       sandbox="existing" if stage == "get" else None,
                       delete_on_exit=False, timeout=3, ready_timeout_seconds=3)

    def sandbox_proto():
        sandbox = openshell_pb2.Sandbox()
        sandbox.metadata.id = "native-id"
        sandbox.metadata.name = "existing"
        sandbox.metadata.workspace = "default"
        sandbox.status.phase = openshell_pb2.SANDBOX_PHASE_READY
        return sandbox

    class Service(openshell_pb2_grpc.OpenShellServicer):
        async def gate(self, current_stage, request, context):
            requests.append((current_stage, request.workspace))
            channels.append(ctx._client._channel)
            if current_stage == stage and failure != "success":
                if failure == "rpc":
                    await context.abort(grpc.StatusCode.PERMISSION_DENIED, "native-entry-sentinel")
                blocked.set()
                await asyncio.Event().wait()

        async def CreateSandbox(self, request, context):
            await self.gate("create", request, context)
            return openshell_pb2.SandboxResponse(sandbox=sandbox_proto())

        async def GetSandbox(self, request, context):
            current_stage = "get" if stage == "get" and len(requests) == 0 else "ready"
            await self.gate(current_stage, request, context)
            return openshell_pb2.SandboxResponse(sandbox=sandbox_proto())

    server = grpc.aio.server()
    openshell_pb2_grpc.add_OpenShellServicer_to_server(Service(), server)
    port = server.add_insecure_port("127.0.0.1:0")
    assert port != 0
    await server.start()
    gateway_dir = tmp_path / "openshell" / "gateways" / "native-entry"
    gateway_dir.mkdir(parents=True)
    (gateway_dir / "metadata.json").write_text(json.dumps({
        "gateway_endpoint": f"http://127.0.0.1:{port}"}))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
    monkeypatch.delenv("OPENSHELL_GATEWAY", raising=False)

    async def enter():
        async with ctx:
            assert ctx.id == "native-id"
            assert channels[-1].get_state() == grpc.ChannelConnectivity.READY

    task = asyncio.create_task(enter())
    try:
        if failure == "cancel":
            await asyncio.wait_for(blocked.wait(), timeout=3)
            task.cancel()
            with pytest.raises(asyncio.CancelledError):
                await task
        elif failure == "rpc":
            with pytest.raises(grpc.aio.AioRpcError) as raised:
                await task
            assert raised.value.code() == grpc.StatusCode.PERMISSION_DENIED
            assert raised.value.details() == "native-entry-sentinel"
        else:
            await task
        assert channels, "real RPC handler was not reached"
        assert all(workspace == "default" for _, workspace in requests)
        assert channels[-1].get_state() == grpc.ChannelConnectivity.SHUTDOWN
        assert ctx._client is None
        assert ctx._session is None
    finally:
        if not task.done():
            task.cancel()
            try:
                await task
            except asyncio.CancelledError:
                pass
        for channel in channels:
            await channel.close()
        await server.stop(0)
