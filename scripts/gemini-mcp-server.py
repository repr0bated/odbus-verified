#!/usr/bin/env python3
"""
MCP Server for Gemini API via Antigravity Proxy
Provides Gemini capabilities through Model Context Protocol
"""
import os
import json
import asyncio
import time
from typing import Any, Optional
from mcp.server import Server
from mcp.types import Tool, TextContent
from openai import AsyncOpenAI
import httpx

# Configuration
GEMINI_BASE_URL = os.getenv("GEMINI_BASE_URL", "https://cloudcode-pa.googleapis.com/v1internal")
GEMINI_API_KEY = os.getenv("GEMINI_API_KEY")
GEMINI_MODEL = os.getenv("GEMINI_MODEL", "gemini-2.0-flash")

def get_gemini_token():
    """Read Gemini CLI OAuth token"""
    try:
        creds_path = os.path.expanduser("~/.gemini/oauth_creds.json")
        if os.path.exists(creds_path):
            with open(creds_path, "r") as f:
                creds = json.load(f)
                token = creds.get("access_token")
                expiry = creds.get("expiry_date", 0)
                # Check if token is still valid (with 5 min buffer)
                if token and expiry > (time.time() * 1000) + 300000:
                    return token
    except Exception:
        pass
    return None

def get_auth_headers():
    """Get headers for Cloud AI Companion API"""
    token = get_gemini_token() or GEMINI_API_KEY
    if not token:
        return {}
    
    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
        "User-Agent": "google-cloud-code-vscode/1.22.0 (GPN:Cloud Code for VS Code) vscode/1.85.0 (linux; x64)",
        "x-goog-api-client": "gl-rust/1.76.0 gax/2.12.0 gapic/1.0.0",
        "x-client-data": "eyJpc0lkZSI6dHJ1ZSwiaWRlVHlwZSI6InZzY29kZSIsImlkZVZlcnNpb24iOiIxLjg1LjAiLCJwbHVnaW5WZXJzaW9uIjoiMS4yMi4wIn0="
    }
    return headers

async def call_cloud_ai(prompt: str, model: str = GEMINI_MODEL):
    """Call Cloud AI Companion directly"""
    headers = get_auth_headers()
    if not headers:
        return "Error: No authentication found. Please run 'gemini' CLI or set GEMINI_API_KEY."
    
    project = os.getenv("GOOGLE_CLOUD_PROJECT", "operation-dbus")
    url = f"{GEMINI_BASE_URL}/generateContent"
    
    body = {
        "model": model,
        "project": project,
        "request": {
            "contents": [{"role": "user", "parts": [{"text": prompt}]}],
            "generationConfig": {
                "temperature": 0.7,
                "maxOutputTokens": 8192,
            }
        }
    }
    
    async with httpx.AsyncClient(timeout=120.0) as client:
        try:
            resp = await client.post(url, headers=headers, json=body)
            if resp.status_code != 200:
                return f"Error: API returned {resp.status_code}: {resp.text}"
            
            data = resp.json()
            candidates = data.get("response", {}).get("candidates", [])
            if not candidates:
                return "Error: No response candidates returned."
            
            parts = candidates[0].get("content", {}).get("parts", [])
            text = "".join(p.get("text", "") for p in parts)
            return text
        except Exception as e:
            return f"Error calling Gemini: {str(e)}"

# Initialize OpenAI client as fallback
client = AsyncOpenAI(
    base_url=os.getenv("OPENAI_BASE_URL", "http://127.0.0.1:8045/v1"),
    api_key=GEMINI_API_KEY or "none"
)

# Create MCP server
app = Server("gemini-proxy")

@app.list_tools()
async def list_tools() -> list[Tool]:
    """List available Gemini tools"""
    return [
        Tool(
            name="gemini_chat",
            description="Chat with Gemini AI model through Antigravity proxy",
            inputSchema={
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Message to send to Gemini"
                    },
                    "model": {
                        "type": "string",
                        "description": "Model to use (default: gemini-2.0-flash-exp)",
                        "enum": ["gemini-2.0-flash-exp", "gemini-1.5-pro", "gemini-1.5-flash"]
                    },
                    "system": {
                        "type": "string",
                        "description": "System prompt (optional)"
                    },
                    "temperature": {
                        "type": "number",
                        "description": "Temperature 0-2 (default: 1.0)"
                    }
                },
                "required": ["message"]
            }
        ),
        Tool(
            name="gemini_analyze_code",
            description="Analyze code using Gemini's code understanding",
            inputSchema={
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "Code to analyze"
                    },
                    "language": {
                        "type": "string",
                        "description": "Programming language"
                    },
                    "task": {
                        "type": "string",
                        "description": "What to do (review, explain, optimize, debug)",
                        "enum": ["review", "explain", "optimize", "debug", "document"]
                    }
                },
                "required": ["code", "task"]
            }
        ),
        Tool(
            name="gemini_generate_code",
            description="Generate code using Gemini",
            inputSchema={
                "type": "object",
                "properties": {
                    "description": {
                        "type": "string",
                        "description": "What code to generate"
                    },
                    "language": {
                        "type": "string",
                        "description": "Programming language"
                    },
                    "context": {
                        "type": "string",
                        "description": "Additional context (optional)"
                    }
                },
                "required": ["description", "language"]
            }
        )
    ]

@app.call_tool()
async def call_tool(name: str, arguments: Any) -> list[TextContent]:
    """Handle tool calls"""
    
    if name == "gemini_chat":
        return await gemini_chat(arguments)
    elif name == "gemini_analyze_code":
        return await gemini_analyze_code(arguments)
    elif name == "gemini_generate_code":
        return await gemini_generate_code(arguments)
    else:
        return [TextContent(type="text", text=f"Unknown tool: {name}")]

async def gemini_chat(args: dict) -> list[TextContent]:
    """Chat with Gemini"""
    message = args["message"]
    model = args.get("model", GEMINI_MODEL)
    system = args.get("system")
    
    prompt = message
    if system:
        prompt = f"System: {system}\n\nUser: {message}"
    
    content = await call_cloud_ai(prompt, model)
    return [TextContent(type="text", text=content)]

async def gemini_analyze_code(args: dict) -> list[TextContent]:
    """Analyze code with Gemini"""
    code = args["code"]
    language = args.get("language", "unknown")
    task = args["task"]
    
    task_prompts = {
        "review": "Review this code for bugs, security issues, and best practices:",
        "explain": "Explain what this code does in detail:",
        "optimize": "Suggest optimizations for this code:",
        "debug": "Help debug this code and identify potential issues:",
        "document": "Generate documentation for this code:"
    }
    
    prompt = f"""{task_prompts.get(task, 'Analyze this code:')}

Language: {language}

```{language}
{code}
```

Provide a detailed analysis."""
    
    return await gemini_chat({"message": prompt, "model": GEMINI_MODEL})

async def gemini_generate_code(args: dict) -> list[TextContent]:
    """Generate code with Gemini"""
    description = args["description"]
    language = args["language"]
    context = args.get("context", "")
    
    prompt = f"""Generate {language} code for the following:

{description}

{f'Context: {context}' if context else ''}

Provide clean, well-commented code following best practices."""
    
    return await gemini_chat({"message": prompt, "model": GEMINI_MODEL})

async def main():
    """Run the MCP server"""
    from mcp.server.stdio import stdio_server
    
    async with stdio_server() as (read_stream, write_stream):
        await app.run(
            read_stream,
            write_stream,
            app.create_initialization_options()
        )

if __name__ == "__main__":
    asyncio.run(main())
