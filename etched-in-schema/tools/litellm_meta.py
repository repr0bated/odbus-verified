"""Meta model via LiteLLM (OpenAI-compatible).

Uses META_API_KEY / META_MODEL / META_MODEL_BASE_URL from the environment
(~/.bash_secrets for dev, /home/3tched/.config/etched/env for the service).
Model reasons before answering: keep max_tokens generous (800+).
"""
import os

import litellm


def complete(prompt: str, *, max_tokens: int = 800) -> str:
    resp = litellm.completion(
        model="openai/" + os.environ["META_MODEL"],
        api_base=os.environ["META_MODEL_BASE_URL"],
        api_key=os.environ["META_API_KEY"],
        messages=[{"role": "user", "content": prompt}],
        max_tokens=max_tokens,
    )
    return resp.choices[0].message.content


if __name__ == "__main__":
    import sys

    print(complete(sys.argv[1] if len(sys.argv) > 1 else "Reply with exactly: litellm-meta-ok"))
