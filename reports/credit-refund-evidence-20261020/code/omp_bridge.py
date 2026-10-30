"""LiteLLM custom provider that routes chat completions through OMP's auth gateway.

OMP keeps the OAuth credentials and refresh lifecycle. This module only speaks the
JSONL protocol of `omp auth-gateway stdio`, so no provider tokens are copied.

Model strings: `omp/<OMP selector>`, e.g. `omp/anthropic/claude-fable-5-1`.
Non-streaming chat completions only.
"""

import asyncio
import concurrent.futures
import itertools
import json
import subprocess
import threading
from typing import Any

import litellm
from litellm import CustomLLM
from litellm.llms.custom_llm import CustomLLMError
from litellm.types.utils import ModelResponse

OMP_BINARY = "/home/jeremy/.local/share/pi-router/omp-18.10.4"
PROVIDER = "omp"
FORWARDED_PARAMS = ("temperature", "top_p", "max_tokens", "stop", "tools", "tool_choice", "reasoning_effort")
STDOUT_LIMIT = 64 * 1024 * 1024  # catalog frames exceed asyncio's 64 KiB default
REQUEST_TIMEOUT_S = 600


class _OmpGateway:
    """One long-lived `omp auth-gateway stdio` process hosted on a private event loop.

    Requests can be submitted from any event loop or thread; responses are correlated by id.
    """

    def __init__(self) -> None:
        self._loop = asyncio.new_event_loop()
        threading.Thread(target=self._loop.run_forever, name="omp-gateway", daemon=True).start()
        self._proc: asyncio.subprocess.Process | None = None
        self._reader: asyncio.Task | None = None
        self._pending: dict[str, asyncio.Future] = {}
        self._ids = itertools.count(1)
        self._start_lock: asyncio.Lock | None = None  # created on first use, on the gateway loop

    def submit(self, coro) -> concurrent.futures.Future:
        return asyncio.run_coroutine_threadsafe(coro, self._loop)

    async def request(self, path: str, body: dict[str, Any]) -> tuple[int, Any]:
        await self._ensure_started()
        request_id = f"omp-{next(self._ids)}"
        future = self._loop.create_future()
        self._pending[request_id] = future
        assert self._proc is not None and self._proc.stdin is not None
        self._proc.stdin.write((json.dumps({"id": request_id, "path": path, "body": body}) + "\n").encode())
        await self._proc.stdin.drain()
        event = await future
        return int(event["status"]), event.get("body")

    async def _ensure_started(self) -> None:
        if self._start_lock is None:
            self._start_lock = asyncio.Lock()
        async with self._start_lock:
            if self._proc is not None and self._proc.returncode is None:
                return
            proc = await asyncio.create_subprocess_exec(
                OMP_BINARY,
                "auth-gateway",
                "stdio",
                stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                limit=STDOUT_LIMIT,
            )
            assert proc.stdout is not None
            ready = json.loads(await proc.stdout.readline())
            if not ready.get("ready"):
                raise CustomLLMError(status_code=503, message="omp auth-gateway did not report ready")
            self._proc = proc
            self._reader = self._loop.create_task(self._read_loop(proc))

    async def _read_loop(self, proc: asyncio.subprocess.Process) -> None:
        assert proc.stdout is not None
        while True:
            line = await proc.stdout.readline()
            if not line:
                break
            event = json.loads(line)
            future = self._pending.pop(event.get("id"), None)
            if future is not None and not future.done():
                future.set_result(event)
        for future in self._pending.values():
            if not future.done():
                future.set_exception(CustomLLMError(status_code=502, message="omp auth-gateway exited"))
        self._pending.clear()


_gateway: _OmpGateway | None = None
_gateway_lock = threading.Lock()


def _get_gateway() -> _OmpGateway:
    global _gateway
    with _gateway_lock:
        if _gateway is None:
            _gateway = _OmpGateway()
        return _gateway


def _build_body(model: str, messages: list, optional_params: dict) -> tuple[str, dict[str, Any]]:
    selector = model[len(PROVIDER) + 1 :] if model.startswith(f"{PROVIDER}/") else model
    body: dict[str, Any] = {"model": selector, "messages": messages}
    for key in FORWARDED_PARAMS:
        if optional_params.get(key) is not None:
            body[key] = optional_params[key]
    return selector, body


def _to_model_response(status: int, payload: Any) -> ModelResponse:
    if status != 200:
        error = payload.get("error") if isinstance(payload, dict) else None
        if isinstance(error, dict):
            message = f"{error.get('type', 'error')}: {error.get('message', '')}"
        else:
            message = str(payload)[:500]
        raise CustomLLMError(status_code=status, message=message)
    return ModelResponse(**payload)


class OmpGatewayLLM(CustomLLM):
    def completion(
        self,
        model,
        messages,
        api_base,
        custom_prompt_dict,
        model_response,
        print_verbose,
        encoding,
        api_key,
        logging_obj,
        optional_params,
        acompletion=None,
        litellm_params=None,
        logger_fn=None,
        headers={},
        timeout=None,
        client=None,
    ):
        _, body = _build_body(model, messages, optional_params)
        gateway = _get_gateway()
        status, payload = gateway.submit(gateway.request("/v1/chat/completions", body)).result(
            timeout=timeout or REQUEST_TIMEOUT_S
        )
        return _to_model_response(status, payload)

    async def acompletion(
        self,
        model,
        messages,
        api_base,
        custom_prompt_dict,
        model_response,
        print_verbose,
        encoding,
        api_key,
        logging_obj,
        optional_params,
        acompletion=None,
        litellm_params=None,
        logger_fn=None,
        headers={},
        timeout=None,
        client=None,
    ):
        _, body = _build_body(model, messages, optional_params)
        gateway = _get_gateway()
        status, payload = await asyncio.wrap_future(gateway.submit(gateway.request("/v1/chat/completions", body)))
        return _to_model_response(status, payload)

    def streaming(self, *args, **kwargs):
        raise CustomLLMError(status_code=400, message="omp bridge: streaming is not supported")

    async def astreaming(self, *args, **kwargs):
        raise CustomLLMError(status_code=400, message="omp bridge: streaming is not supported")


def register() -> None:
    if not any(item["provider"] == PROVIDER for item in litellm.custom_provider_map):
        litellm.custom_provider_map.append({"provider": PROVIDER, "custom_handler": OmpGatewayLLM()})


register()
