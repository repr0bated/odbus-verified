"""LiteLLM custom provider `hermes`: Nous Portal chat completions using Hermes-held OAuth.

Each call resolves the Nous inference credential through Hermes's own resolver (which refreshes
the token) in a subprocess under Hermes's Python. The key stays in this process and is never logged.
Supports non-streaming calls and native asynchronous SSE streaming.
"""

import asyncio
import json
import subprocess
from typing import Any

import httpx
import litellm
from litellm import CustomLLM
from litellm.llms.custom_llm import CustomLLMError
from litellm.types.utils import ModelResponse, ModelResponseStream

HERMES_PYTHON = "/home/jeremy/.hermes/tools/python-3.14.7+20260901-linux-x64/bin/python3"
HERMES_AGENT = "/home/jeremy/.hermes/hermes-agent"
PROVIDER = "hermes"
FORWARDED_PARAMS = ("temperature", "top_p", "max_tokens", "stop", "tools", "tool_choice", "reasoning_effort")
REQUEST_TIMEOUT_S = 600

_CREDENTIAL_SCRIPT = (
    "import os,sys,json; os.environ.pop('PYTHONHOME',None); os.environ.pop('PYTHONPATH',None);"
    f"sys.path.insert(0,{HERMES_AGENT!r}); sys._hermes_pin_default_home=True; import hermes_bootstrap;"
    "from hermes_cli.auth_nous import resolve_nous_runtime_credentials as r;"
    "c=r(); print(json.dumps({'base_url': c['base_url'], 'api_key': c['api_key']}))"
)


def _nous_credentials() -> tuple[str, str]:
    proc = subprocess.run(
        [HERMES_PYTHON, "-I", "-c", _CREDENTIAL_SCRIPT],
        capture_output=True,
        text=True,
        timeout=120,
    )
    if proc.returncode != 0:
        raise CustomLLMError(status_code=401, message=f"hermes bridge: credential resolve failed (rc={proc.returncode})")
    creds = json.loads(proc.stdout.strip().splitlines()[-1])
    return creds["base_url"], creds["api_key"]


def _build_body(model: str, messages: list, optional_params: dict) -> dict[str, Any]:
    selector = model[len(PROVIDER) + 1 :] if model.startswith(f"{PROVIDER}/") else model
    body: dict[str, Any] = {"model": selector, "messages": messages}
    for key in FORWARDED_PARAMS:
        if optional_params.get(key) is not None:
            body[key] = optional_params[key]
    return body


def _post(body: dict[str, Any], timeout: float | None) -> ModelResponse:
    base_url, api_key = _nous_credentials()
    response = httpx.post(
        f"{base_url}/chat/completions",
        json=body,
        headers={"Authorization": f"Bearer {api_key}"},
        timeout=timeout or REQUEST_TIMEOUT_S,
    )
    try:
        payload = response.json()
    except ValueError:
        payload = {"error": {"type": "non_json", "message": response.text[:500]}}
    if response.status_code != 200:
        error = payload.get("error") if isinstance(payload, dict) else None
        if isinstance(error, dict):
            message = f"{error.get('type', 'error')}: {error.get('message', '')}"
        else:
            message = str(payload)[:500]
        raise CustomLLMError(status_code=response.status_code, message=message)
    return ModelResponse(**payload)


class HermesNousLLM(CustomLLM):
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
        return _post(_build_body(model, messages, optional_params), timeout)

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
        return await asyncio.to_thread(_post, _build_body(model, messages, optional_params), timeout)

    def streaming(self, *args, **kwargs):
        raise CustomLLMError(status_code=400, message="hermes bridge: streaming is not supported")

    async def astreaming(
        self, model, messages, api_base, custom_prompt_dict, model_response,
        print_verbose, encoding, api_key, logging_obj, optional_params,
        acompletion=None, litellm_params=None, logger_fn=None, headers=None,
        timeout=None, client=None,
    ):
        base_url, credential = await asyncio.to_thread(_nous_credentials)
        body = _build_body(model, messages, optional_params)
        body.update(stream=True, stream_options={"include_usage": True})
        finished = False
        async with httpx.AsyncClient(timeout=timeout or REQUEST_TIMEOUT_S) as transport:
            async with transport.stream(
                "POST", f"{base_url}/chat/completions", json=body,
                headers={"Authorization": f"Bearer {credential}"},
            ) as response:
                if response.status_code != 200:
                    raise CustomLLMError(
                        response.status_code, f"hermes stream: HTTP {response.status_code}"
                    )
                data = []
                async for line in response.aiter_lines():
                    if line.startswith("data:"):
                        data.append(line[5:].lstrip(" "))
                    elif line == "" and data:
                        event = "\n".join(data)
                        data.clear()
                        if event == "[DONE]":
                            if not finished:
                                raise CustomLLMError(502, "hermes stream ended without finish_reason")
                            return
                        payload = json.loads(event)
                        if "error" in payload:
                            raise CustomLLMError(502, "hermes upstream stream error")
                        choices = payload.get("choices", [])
                        finished = finished or any(c.get("finish_reason") for c in choices)
                        if choices:
                            yield ModelResponseStream(**payload)
                        elif payload.get("usage"):
                            yield {
                                "text": "", "is_finished": False, "finish_reason": "",
                                "usage": payload["usage"],
                            }
        if not finished:
            raise CustomLLMError(502, "hermes stream disconnected before completion")


def register() -> None:
    if not any(item["provider"] == PROVIDER for item in litellm.custom_provider_map):
        litellm.custom_provider_map.append({"provider": PROVIDER, "custom_handler": HermesNousLLM()})


register()
