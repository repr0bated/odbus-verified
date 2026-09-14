import argparse
import asyncio
import os
import re
from pathlib import Path
from google import genai
from google.genai import types
import random

def parse_args():
    parser = argparse.ArgumentParser(description="Generate LLM specs for crates.")
    parser.add_argument("--crate", type=str, help="Specific crate to process")
    parser.add_argument("--dry-run", action="store_true", help="Print prompt construction instead of calling API")
    return parser.parse_args()

def discover_crates(crates_dir="crates"):
    path = Path(crates_dir)
    crates = []
    if not path.exists():
        return crates
    for p in path.iterdir():
        if p.is_dir() and p.name.startswith("op-"):
            crates.append(p.name)
    return crates

def read_file_safe(path):
    try:
        with open(path, "r", encoding="utf-8") as f:
            return f.read()
    except Exception as e:
        return f"Error reading {path}: {e}"

def assemble_context(crate_name, base_dir="."):
    base_path = Path(base_dir)
    crate_path = base_path / "crates" / crate_name
    
    context = []
    
    # AGENTS.md
    agents_md_path = base_path / "AGENTS.md"
    if agents_md_path.exists():
        agents_md = read_file_safe(agents_md_path)
        context.append(f"--- AGENTS.md ---\n{agents_md}\n")
    
    # Cargo.toml
    cargo_toml_path = crate_path / "Cargo.toml"
    if cargo_toml_path.exists():
        cargo_toml = read_file_safe(cargo_toml_path)
        context.append(f"--- {crate_name}/Cargo.toml ---\n{cargo_toml}\n")
    
    # .rs files in src/
    src_path = crate_path / "src"
    if src_path.exists():
        for root, _, files in os.walk(src_path):
            for file in files:
                if file.endswith(".rs"):
                    file_path = Path(root) / file
                    content = read_file_safe(file_path)
                    rel_path = file_path.relative_to(crate_path)
                    context.append(f"--- {crate_name}/{rel_path} ---\n{content}\n")
                    
    return "\n".join(context)

def get_few_shot_examples(base_dir="."):
    base_path = Path(base_dir)
    examples = []
    
    for example_crate in ["op-web", "op-services"]:
        example_dir = base_path / ".kiro" / "specs" / example_crate
        if example_dir.exists():
            req_path = example_dir / "requirements.md"
            design_path = example_dir / "design.md"
            tasks_path = example_dir / "tasks.md"
            
            req = read_file_safe(req_path) if req_path.exists() else "No requirements found."
            design = read_file_safe(design_path) if design_path.exists() else "No design found."
            tasks = read_file_safe(tasks_path) if tasks_path.exists() else "No tasks found."
            
            examples.append(f"Example for {example_crate}:\n\n<requirements>\n{req}\n</requirements>\n\n<design>\n{design}\n</design>\n\n<tasks>\n{tasks}\n</tasks>\n")
            
    return "\n\n".join(examples)

def build_prompt(crate_name, context, examples):
    return f"""You are an expert Rust software architect.
Your task is to generate the requirements, design, and implementation tasks for the `{crate_name}` crate.
Please output valid XML tags `<requirements>`, `<design>`, and `<tasks>` containing the markdown content for each respective document.

Here are the project guidelines and the current context of the crate:
<context>
{context}
</context>

Here are some few-shot examples of what the output should look like:
<examples>
{examples}
</examples>

Please provide your output for the `{crate_name}` crate now.
"""

async def call_llm_with_backoff(prompt, max_retries=5, dry_run=False):
    if dry_run:
        print("--- DRY RUN: LLM Prompt (Truncated) ---")
        print(prompt[:500] + "...\n[PROMPT TRUNCATED]")
        print("--- END DRY RUN ---")
        return "<requirements>\n# Requirements for dry run\n- requirement 1\n</requirements>\n<design>\n# Design for dry run\n- design 1\n</design>\n<tasks>\n# Tasks for dry run\n- [ ] task 1\n</tasks>"

    # Prevent API calls that incur charges unless explicitly allowed
    if not os.environ.get("ALLOW_PAID_API_CALLS"):
        print("Error: API calls that could incur charges are disabled. Set ALLOW_PAID_API_CALLS=1 to proceed.")
        return None

    client = genai.Client()
    
    for attempt in range(max_retries):
        try:
            response = await client.aio.models.generate_content(
                model='gemini-2.5-flash',
                contents=prompt,
            )
            return response.text
        except Exception as e:
            if "429" in str(e) or "Too Many Requests" in str(e):
                delay = (2 ** attempt) + random.uniform(0, 1)
                print(f"Rate limited. Retrying in {delay:.2f} seconds (Attempt {attempt + 1}/{max_retries})")
                await asyncio.sleep(delay)
            else:
                print(f"LLM API Call failed: {e}")
                if attempt == max_retries - 1:
                    return None
                delay = (2 ** attempt) + random.uniform(0, 1)
                print(f"Error occurred. Retrying in {delay:.2f} seconds (Attempt {attempt + 1}/{max_retries})")
                await asyncio.sleep(delay)
    
    print("Max retries exceeded.")
    return None

def extract_and_write_specs(crate_name, response_text, base_dir="."):
    if not response_text:
        print(f"[{crate_name}] No response text to parse.")
        return False
        
    base_path = Path(base_dir)
    out_dir = base_path / ".kiro" / "specs" / crate_name
    out_dir.mkdir(parents=True, exist_ok=True)
    
    tags = ["requirements", "design", "tasks"]
    success = True
    
    for tag in tags:
        # Match content inside <tag> ... </tag>
        # The re.DOTALL flag makes '.' match newlines
        match = re.search(f"<{tag}>(.*?)</{tag}>", response_text, re.IGNORECASE | re.DOTALL)
        if match:
            content = match.group(1).strip()
            out_file = out_dir / f"{tag}.md"
            try:
                with open(out_file, "w", encoding="utf-8") as f:
                    f.write(content + "\n")
                print(f"[{crate_name}] Wrote {out_file}")
            except Exception as e:
                print(f"[{crate_name}] Error writing {out_file}: {e}")
                success = False
        else:
            print(f"[{crate_name}] Warning: <{tag}> tag not found in response.")
            success = False
            
    return success

async def process_crate(crate, examples, dry_run=False):
    print(f"[{crate}] Starting processing...")
    context = assemble_context(crate)
    prompt = build_prompt(crate, context, examples)
    
    response = await call_llm_with_backoff(prompt, dry_run=dry_run)
    if response:
        print(f"[{crate}] Generated response ({len(response)} characters)")
        extract_and_write_specs(crate, response)
    else:
        print(f"[{crate}] Failed to get response.")

async def main_async():
    args = parse_args()
    
    crates = [args.crate] if args.crate else discover_crates()
    
    if not crates:
        print("No crates found.")
        return
        
    examples = get_few_shot_examples()
    
    # Run all crate processing in parallel
    tasks = [process_crate(crate, examples, args.dry_run) for crate in crates]
    await asyncio.gather(*tasks)

def main():
    asyncio.run(main_async())

if __name__ == "__main__":
    main()
