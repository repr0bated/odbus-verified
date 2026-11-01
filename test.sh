curl -X POST https://api.meta.ai/v1/responses \
  -H "Authorization: Bearer $MODEL_API_KEY" \
  -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -d '{
  "model": "muse-spark-1.3-contributor",
  "input": [
    {
      "role": "user",
      "content": "why does the model work in this chat but not in api calls"
    },
    {
      "role": "assistant",
      "content": "This is the #1 API gotcha — you'\''re almost never sending the *same* thing in the API as this chat is sending behind the scenes.\n\nThis chat app adds a lot of hidden stuff for you. When you call the API yourself, you have to add it manually.\n\nHere are the differences that break 95% of people:\n\n**1. The API is stateless, this chat is not**\nHere I remember the whole conversation automatically.\n\nIn the API you have to re-send the full history every time:\n```\nsystem: ...\nuser: message 1\nassistant: answer 1\nuser: message 2\n```\nIf you only send the last `user:` message, it will act dumb / lose instructions.\n\n**2. You'\''re not using the same system prompt**\nThis chat runs with a hidden system instruction like `You are Muse Spark, a friendly AI Assistant...` plus formatting, safety, and style instructions.\n\nIn the API if your `system` is empty or different, you'\''ll get a totally different personality / quality. Copy-paste the system prompt from here into your `system` parameter.\n\n**3. You'\''re not using the same model version**\nThis chat usually says \"latest\" and auto-updates.\n\nAPI requires an exact ID like `muse-spark-...-001`, `gemini-1.5-pro-002`, `gpt-4o-2024-...`, etc. If you hardcoded an old pinned version, you'\''re literally talking to a different model.\n\nCheck: What model ID are you passing in code?\n\n**4. Default generation settings are different**\nThe chat UI uses tuned defaults for:\n`temperature, top_p, top_k, max_output_tokens`\n\nAPI SDKs use *their own* defaults, often `temperature=0` or `max_tokens= low`. That alone can make it look broken — too short, too repetitive, or too random.\n\nSet them explicitly to match.\n\n**5. This chat has tools turned on, your API doesn'\''t**\nThis chat may have web search, code execution, file retrieval, function-calling enabled by default.\n\nIn the API those are OFF unless you pass `tools=[...]`, `grounding`, `code_interpreter`, etc. So tasks that need calculation / browsing will work here and fail there.\n\n**6. Safety filters / project settings**\nPlaygrounds often run in \"permissive for testing\" mode. API calls run under *your* project settings with stricter safety blocks, quota limits, or no access to that model in that region. A silent block looks like \"it doesn'\''t work.\"\n\n### How to make them identical\n\n1. Look for a **View Code / Get Code / <>** button in this chat. Most playgrounds (OpenAI, Anthropic, Google AI Studio, Vertex) can export the *exact* Python / cURL it'\''s using. Use that as your starting point, don'\''t write it from scratch.\n2. Log what you send: print the exact `model, messages, system_instruction, temperature, top_p, max_tokens, tools`\n3. Start with a minimal repro: 1 system + 1 user message, temperature 0.7, max_tokens 1024, and expand from there.\n\n> Tell me this and I can pinpoint it:\n> 1. Which provider/SDK are you using?\n> 2. What'\''s the model name in the API call?\n> 3. What happens in API — error message? Empty? Bad quality? Share your (redacted, no key) code snippet.\n\n99% of the time once we compare the two payloads, the difference jumps out."
    },
    {
      "role": "user",
      "content": "i pasete exacty what  </> says and get"
    }
  ],
  "stream": true,
  "temperature": 1,
  "max_output_tokens": 32000,
  "top_p": 1,
  "reasoning": {
    "effort": "medium"
  }
}'curl -X POST https://api.meta.ai/v1/responses \
  -H "Authorization: Bearer $MODEL_API_KEY" \
  -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -d '{
  "model": "muse-spark-1.3-contributor",
  "input": [
    {
      "role": "user",
      "content": "why does the model work in this chat but not in api calls"
    },
    {
      "role": "assistant",
      "content": "This is the #1 API gotcha — you'\''re almost never sending the *same* thing in the API as this chat is sending behind the scenes.\n\nThis chat app adds a lot of hidden stuff for you. When you call the API yourself, you have to add it manually.\n\nHere are the differences that break 95% of people:\n\n**1. The API is stateless, this chat is not**\nHere I remember the whole conversation automatically.\n\nIn the API you have to re-send the full history every time:\n```\nsystem: ...\nuser: message 1\nassistant: answer 1\nuser: message 2\n```\nIf you only send the last `user:` message, it will act dumb / lose instructions.\n\n**2. You'\''re not using the same system prompt**\nThis chat runs with a hidden system instruction like `You are Muse Spark, a friendly AI Assistant...` plus formatting, safety, and style instructions.\n\nIn the API if your `system` is empty or different, you'\''ll get a totally different personality / quality. Copy-paste the system prompt from here into your `system` parameter.\n\n**3. You'\''re not using the same model version**\nThis chat usually says \"latest\" and auto-updates.\n\nAPI requires an exact ID like `muse-spark-...-001`, `gemini-1.5-pro-002`, `gpt-4o-2024-...`, etc. If you hardcoded an old pinned version, you'\''re literally talking to a different model.\n\nCheck: What model ID are you passing in code?\n\n**4. Default generation settings are different**\nThe chat UI uses tuned defaults for:\n`temperature, top_p, top_k, max_output_tokens`\n\nAPI SDKs use *their own* defaults, often `temperature=0` or `max_tokens= low`. That alone can make it look broken — too short, too repetitive, or too random.\n\nSet them explicitly to match.\n\n**5. This chat has tools turned on, your API doesn'\''t**\nThis chat may have web search, code execution, file retrieval, function-calling enabled by default.\n\nIn the API those are OFF unless you pass `tools=[...]`, `grounding`, `code_interpreter`, etc. So tasks that need calculation / browsing will work here and fail there.\n\n**6. Safety filters / project settings**\nPlaygrounds often run in \"permissive for testing\" mode. API calls run under *your* project settings with stricter safety blocks, quota limits, or no access to that model in that region. A silent block looks like \"it doesn'\''t work.\"\n\n### How to make them identical\n\n1. Look for a **View Code / Get Code / <>** button in this chat. Most playgrounds (OpenAI, Anthropic, Google AI Studio, Vertex) can export the *exact* Python / cURL it'\''s using. Use that as your starting point, don'\''t write it from scratch.\n2. Log what you send: print the exact `model, messages, system_instruction, temperature, top_p, max_tokens, tools`\n3. Start with a minimal repro: 1 system + 1 user message, temperature 0.7, max_tokens 1024, and expand from there.\n\n> Tell me this and I can pinpoint it:\n> 1. Which provider/SDK are you using?\n> 2. What'\''s the model name in the API call?\n> 3. What happens in API — error message? Empty? Bad quality? Share your (redacted, no key) code snippet.\n\n99% of the time once we compare the two payloads, the difference jumps out."
    },
    {
      "role": "user",
      "content": "i pasete exacty what  </> says and get"
    }
  ],
  "stream": true,
  "temperature": 1,
  "max_output_tokens": 32000,
  "top_p": 1,
  "reasoning": {
    "effort": "medium"
  }
}'curl -X POST https://api.meta.ai/v1/responses \
  -H "Authorization: Bearer $MODEL_API_KEY" \
  -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -d '{
  "model": "muse-spark-1.3-contributor",
  "input": [
    {
      "role": "user",
      "content": "why does the model work in this chat but not in api calls"
    },
    {
      "role": "assistant",
      "content": "This is the #1 API gotcha — you'\''re almost never sending the *same* thing in the API as this chat is sending behind the scenes.\n\nThis chat app adds a lot of hidden stuff for you. When you call the API yourself, you have to add it manually.\n\nHere are the differences that break 95% of people:\n\n**1. The API is stateless, this chat is not**\nHere I remember the whole conversation automatically.\n\nIn the API you have to re-send the full history every time:\n```\nsystem: ...\nuser: message 1\nassistant: answer 1\nuser: message 2\n```\nIf you only send the last `user:` message, it will act dumb / lose instructions.\n\n**2. You'\''re not using the same system prompt**\nThis chat runs with a hidden system instruction like `You are Muse Spark, a friendly AI Assistant...` plus formatting, safety, and style instructions.\n\nIn the API if your `system` is empty or different, you'\''ll get a totally different personality / quality. Copy-paste the system prompt from here into your `system` parameter.\n\n**3. You'\''re not using the same model version**\nThis chat usually says \"latest\" and auto-updates.\n\nAPI requires an exact ID like `muse-spark-...-001`, `gemini-1.5-pro-002`, `gpt-4o-2024-...`, etc. If you hardcoded an old pinned version, you'\''re literally talking to a different model.\n\nCheck: What model ID are you passing in code?\n\n**4. Default generation settings are different**\nThe chat UI uses tuned defaults for:\n`temperature, top_p, top_k, max_output_tokens`\n\nAPI SDKs use *their own* defaults, often `temperature=0` or `max_tokens= low`. That alone can make it look broken — too short, too repetitive, or too random.\n\nSet them explicitly to match.\n\n**5. This chat has tools turned on, your API doesn'\''t**\nThis chat may have web search, code execution, file retrieval, function-calling enabled by default.\n\nIn the API those are OFF unless you pass `tools=[...]`, `grounding`, `code_interpreter`, etc. So tasks that need calculation / browsing will work here and fail there.\n\n**6. Safety filters / project settings**\nPlaygrounds often run in \"permissive for testing\" mode. API calls run under *your* project settings with stricter safety blocks, quota limits, or no access to that model in that region. A silent block looks like \"it doesn'\''t work.\"\n\n### How to make them identical\n\n1. Look for a **View Code / Get Code / <>** button in this chat. Most playgrounds (OpenAI, Anthropic, Google AI Studio, Vertex) can export the *exact* Python / cURL it'\''s using. Use that as your starting point, don'\''t write it from scratch.\n2. Log what you send: print the exact `model, messages, system_instruction, temperature, top_p, max_tokens, tools`\n3. Start with a minimal repro: 1 system + 1 user message, temperature 0.7, max_tokens 1024, and expand from there.\n\n> Tell me this and I can pinpoint it:\n> 1. Which provider/SDK are you using?\n> 2. What'\''s the model name in the API call?\n> 3. What happens in API — error message? Empty? Bad quality? Share your (redacted, no key) code snippet.\n\n99% of the time once we compare the two payloads, the difference jumps out."
    },
    {
      "role": "user",
      "content": "i pasete exacty what  </> says and get"
    }
  ],
  "stream": true,
  "temperature": 1,
  "max_output_tokens": 32000,
  "top_p": 1,
  "reasoning": {
    "effort": "medium"
  }
}'
