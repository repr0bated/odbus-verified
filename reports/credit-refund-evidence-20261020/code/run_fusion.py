"""Run one full litellm/fusion-1 request against fusion_config.yaml.

Loads SALAD_API_KEY from ~/.bash_secrets in-process (never printed or written to disk).
"""

import asyncio
import argparse
import contextvars
import copy
import hashlib
import json
import os
import re
import secrets
import socket
from pathlib import Path
from datetime import datetime, timezone

import yaml
import litellm
from litellm import Router
from litellm.integrations.custom_logger import CustomLogger

import hermes_bridge  # noqa: F401  registers the `hermes` custom provider

QUESTION = (
    "In under 120 words: when is SQLite a better choice than Postgres for a single-user "
    "desktop app, and when is it not?"
)


class MemberLog(CustomLogger):
    async def async_log_success_event(self, kwargs, response_obj, start_time, end_time):
        print("member ok  ", kwargs.get("model"), flush=True)

    async def async_log_failure_event(self, kwargs, response_obj, start_time, end_time):
        exc = kwargs.get("exception")
        print("member FAIL", kwargs.get("model"), type(exc).__name__, str(exc)[:200], flush=True)


async def main() -> None:
    with open(os.path.expanduser("~/.bash_secrets")) as f:
        secrets = f.read()
    salad_key = re.search(r'SALAD_API_KEY=["\']?([^"\'\s]+)', secrets).group(1)

    with open("fusion_config.yaml") as f:
        config = yaml.safe_load(f)
    for entry in config["model_list"]:
        params = entry["litellm_params"]
        if params.get("api_key") == "os.environ/SALAD_API_KEY":
            params["api_key"] = salad_key

    litellm.callbacks = [MemberLog()]
    router = Router(model_list=config["model_list"])
    response = await router.acompletion(
        model="litellm/fusion-1",
        messages=[{"role": "user", "content": QUESTION}],
    )
    print("RESULT model=", response.model, flush=True)
    print(response.choices[0].message.content, flush=True)


async def run_agent(args) -> None:
    """Let OMP own tools and consume native Fusion streaming on private loopback."""
    import uvicorn
    from fastapi import FastAPI, HTTPException, Request
    from fastapi.responses import StreamingResponse
    import litellm.fusion_router as fusion

    repo = Path(args.repo).resolve(strict=True)
    handoff = Path(args.handoff).resolve(strict=True)
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    text = handoff.read_text()
    prompt, separator, evidence = text.partition("\n---\n")
    if not separator:
        raise ValueError("Handoff must contain its prompt/evidence separator")
    request_text = prompt + "\n\n<evidence>\n" + evidence + "\n</evidence>\n"
    (output / "prompt.txt").write_text(prompt)
    (output / "evidence.txt").write_text(evidence)
    (output / "request.txt").write_text(request_text)
    mandatory = (
        "grpc-expert", "grpc-protocol-expert", "json-render", "ovs-db-analysis",
        "kernel-fuse-diagnosis", "fuse", "btrfs", "rust-memory-optimization",
        "memory-safety-patterns",
    )
    support = [Path(args.context).read_text()]
    for name in mandatory:
        path = repo / ".agents" / "skills" / name / "SKILL.md"
        support.append(f"\n# Mandatory preload: {path}\n\n" + path.read_text())
    support_path = output / "preloaded-context.txt"
    support_path.write_text("\n\n".join(support))
    config = yaml.safe_load(Path(__file__).with_name("fusion_config.yaml").read_text())
    stored = Path("~/.bash_secrets").expanduser().read_text()
    salad_key = re.search(r'SALAD_API_KEY=["\']?([^"\'\s]+)', stored).group(1)
    del stored
    for entry in config["model_list"]:
        params = entry["litellm_params"]
        if params.get("api_key") == "os.environ/SALAD_API_KEY":
            params["api_key"] = salad_key
    del salad_key
    automatic = copy.deepcopy(config["model_list"])
    for entry in automatic:
        if entry["litellm_params"]["model"] == "fusion_router":
            entry["litellm_params"]["fusion_router_config"]["invocation"] = "auto"
    routers = (Router(model_list=config["model_list"]), Router(model_list=automatic))
    current = contextvars.ContextVar("fusion_request")
    original_panel = fusion._panel_messages
    original_analyst = fusion._analyst_messages
    original_payload = fusion._tool_result_payload

    def contextual_messages(messages):
        # One leading system message; no instruction is discarded or demoted.
        ctx = current.get()
        instructions = "\n\n".join(
            m["content"] for m in ctx["messages"]
            if m["role"] in ("system", "developer") and isinstance(m.get("content"), str)
        )
        messages[0]["content"] = instructions + "\n\n" + messages[0]["content"]
        return messages

    def panel_messages(query):
        return contextual_messages(original_panel(
            request_text + "\n\n<current_question>\n" + query + "\n</current_question>"
        ))

    def analyst_messages(query, candidates, max_chars, criteria):
        return contextual_messages(original_analyst(
            request_text + "\n\n<current_question>\n" + query + "\n</current_question>",
            candidates, max_chars, criteria,
        ))

    def capture_payload(*values, **kwargs):
        payload = original_payload(*values, **kwargs)
        (output / f"panel-{current.get()['id']}.json").write_text(json.dumps(payload, indent=2))
        return payload

    fusion._panel_messages = panel_messages
    fusion._analyst_messages = analyst_messages
    fusion._tool_result_payload = capture_payload
    app = FastAPI()
    token = secrets.token_urlsafe(32)
    sequence = 0

    @app.post("/v1/chat/completions")
    async def completion(request: Request):
        nonlocal sequence
        if not secrets.compare_digest(request.headers.get("authorization", ""), "Bearer " + token):
            raise HTTPException(401, "Invalid local runner credential")
        body = await request.json()
        if body.get("model") != "litellm/fusion-1":
            raise HTTPException(400, "Only the requested Fusion alias is available")
        sequence += 1
        request_id = sequence
        wants_stream = body.pop("stream", False)
        body.pop("stream_options", None)
        context_token = current.set({"id": request_id, "messages": body["messages"]})

        started = datetime.now(timezone.utc).isoformat()
        router = routers[int(any(m["role"] == "assistant" for m in body["messages"]))]
        pending = asyncio.create_task(router.acompletion(**body, stream=wants_stream))
        current.reset(context_token)
        print(f"\n[Fusion {request_id}: deliberating]", flush=True)

        def save_receipt(response, hidden, **timing):
            receipt = {
                "request": request_id, "started": started,
                "finished": datetime.now(timezone.utc).isoformat(),
                "model": response.model, "fusion": hidden.get("fusion"),
                "tool_names": [t["function"]["name"] for t in body.get("tools", [])],
                "response": response.model_dump(mode="json", exclude_none=True),
                **timing,
            }
            (output / f"completion-{request_id}.json").write_text(json.dumps(receipt, indent=2))
            return receipt["response"]

        if not wants_stream:
            response = await pending
            return save_receipt(response, getattr(response, "_hidden_params", {}))

        async def stream_events():
            response = None
            chunks = []
            first_chunk = None
            try:
                while not pending.done():
                    done, _ = await asyncio.wait({pending}, timeout=10)
                    if not done:
                        yield ": Fusion deliberation in progress\n\n"
                response = await pending
                print(f"\n[Fusion {request_id}: streaming]", flush=True)
                with (output / f"stream-{request_id}.jsonl").open("w") as transcript:
                    async for chunk in response:
                        timestamp = datetime.now(timezone.utc).isoformat()
                        if first_chunk is None:
                            first_chunk = timestamp
                        chunks.append(chunk)
                        payload = chunk.model_dump(mode="json", exclude_none=True)
                        transcript.write(json.dumps({"at": timestamp, "chunk": payload}) + "\n")
                        transcript.flush()
                        yield "data: " + json.dumps(payload) + "\n\n"
                assembled = litellm.stream_chunk_builder(chunks=chunks, messages=body["messages"])
                save_receipt(
                    assembled, getattr(response, "_hidden_params", {}),
                    first_chunk=first_chunk, stream_chunks=len(chunks),
                )
                yield "data: [DONE]\n\n"
            except Exception as exc:
                error = {"error": {"type": type(exc).__name__, "message": "Fusion stream failed"}}
                (output / f"error-{request_id}.json").write_text(json.dumps(error))
                yield "data: " + json.dumps(error) + "\n\n"
            finally:
                if not pending.done():
                    pending.cancel()
                    await asyncio.gather(pending, return_exceptions=True)
                if response is not None and hasattr(response, "aclose"):
                    await response.aclose()

        return StreamingResponse(stream_events(), media_type="text/event-stream")

    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen()
    port = listener.getsockname()[1]
    server = uvicorn.Server(uvicorn.Config(app, log_level="warning", access_log=False))
    service = asyncio.create_task(server.serve(sockets=[listener]))
    environment = dict(os.environ, FUSION_BRIDGE_TOKEN=token,
                       FUSION_BRIDGE_URL=f"http://127.0.0.1:{port}/v1")
    submitted_prompt = Path(args.continuation).read_text() if args.continuation else request_text
    (output / "submitted-prompt.txt").write_text(submitted_prompt)
    command = [
        args.omp, "--cwd", str(repo), "--add-dir", "/srv/git/3tched.com",
        "--add-dir", "/run/mounts/3tchedFS", "--add-dir", "/run/mounts/threetched",
        "--extension", args.extension,
        "--config", args.config,
        "--model", "consolidation-fusion/litellm/fusion-1",
        "--append-system-prompt", str(support_path),
        "--session-dir", str(output / "sessions"), "--mode", "json", "--print", submitted_prompt,
    ]
    if args.resume:
        command[1:1] = ["--resume", args.resume]
    receipt = {
        "started": datetime.now(timezone.utc).isoformat(), "repo": str(repo),
        "handoff": str(handoff), "source_sha256": hashlib.sha256(text.encode()).hexdigest(),
        "extension": args.extension, "config": args.config, "context": args.context,
        "resume": args.resume, "continuation": args.continuation,
        "transport": "authenticated loopback, native asynchronous SSE; OMP executes tools",
        "invocation": "first required; subsequent tool steps auto",
    }
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2))
    process = None
    try:
        with (output / "agent.jsonl").open("w") as stdout, (output / "agent.stderr").open("w") as stderr:
            process = await asyncio.create_subprocess_exec(
                *command, cwd=repo, env=environment, stdout=asyncio.subprocess.PIPE,
                stderr=stderr, limit=4 * 1024 * 1024,
            )
            receipt["pid"] = process.pid
            (output / "receipt.json").write_text(json.dumps(receipt, indent=2))
            print(f"Fusion tool agent started pid={process.pid} output={output}", flush=True)
            async for line in process.stdout:
                stdout.write(line.decode())
                stdout.flush()
                event = json.loads(line)
                if event.get("type") == "message_update":
                    delta = event.get("assistantMessageEvent", {})
                    if delta.get("type") == "text_delta":
                        print(delta.get("delta", ""), end="", flush=True)
                elif event.get("type") == "tool_execution_start":
                    arguments = event.get("args", {})
                    intent = arguments.get("i", "") if isinstance(arguments, dict) else ""
                    print(f"\n[OMP tool: {event.get('toolName')} {intent}]", flush=True)
                elif event.get("type") == "tool_execution_end":
                    print(f"\n[OMP tool complete: {event.get('toolName')}]", flush=True)
            receipt["exit_code"] = await process.wait()
    finally:
        if process is not None and process.returncode is None:
            process.terminate()
            await process.wait()
        server.should_exit = True
        await service
        listener.close()
        receipt["finished"] = datetime.now(timezone.utc).isoformat()
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt), flush=True)
    if receipt.get("exit_code") != 0:
        raise SystemExit(receipt.get("exit_code", 1))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--agent", action="store_true")
    parser.add_argument("--repo")
    parser.add_argument("--handoff")
    parser.add_argument("--output")
    parser.add_argument("--extension")
    parser.add_argument("--config")
    parser.add_argument("--context")
    parser.add_argument("--omp", default="omp")
    parser.add_argument("--resume")
    parser.add_argument("--continuation")
    args = parser.parse_args()
    if args.agent:
        for name in ("repo", "handoff", "output", "extension", "config", "context"):
            if not getattr(args, name):
                parser.error(f"--{name} is required with --agent")
    asyncio.run(run_agent(args) if args.agent else main())
