#!/usr/bin/env python3
"""Writes and translates Razdor wiki pages with the LOCAL model through Ollama.

Uses the OpenAI-compatible endpoint of Ollama (stdlib only; run with `python3 -I`).

  write      --brief BRIEF.md --source FILE [--source FILE ...] --out PAGE.md
  translate  --in PAGE.md --out EN.md

Both modes send reasoning_effort "none", temperature 0.3, no streaming, and print the token
usage to stderr. Exit status 1 with a message when Ollama cannot be reached or refuses.
"""

import argparse
import json
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

DEFAULT_URL = "http://localhost:11434/v1/chat/completions"
DEFAULT_MODEL = "gemma4-26b-gpu"
TIMEOUT_S = 600

WRITE_SYSTEM = """You write one page of the Razdor player wiki, in Russian.
- Plain, clear player language. Short sentences. No internal jargon.
- The output is Starlight Markdown: YAML frontmatter first (keys `title` and `description`), then the body.
- Use only facts from the sources given by the user. Never invent keys, numbers, menu names or behaviour. If the sources do not say something, leave it out.
- Keep menu and button names exactly as in the sources, in «ёлочки».
- Short sections with `##` headings.
- Use Starlight asides where they help: `:::note`, `:::tip` and `:::caution`, each closed with `:::`.
- Refer to screenshots only by the names given in the brief, as `../../../assets/shots/<name>.webp` in Markdown image syntax.
- Output only the page itself, with no commentary before or after it."""

TRANSLATE_SYSTEM = """You translate one page of the Razdor player wiki from Russian into English, for players.
- Keep the Markdown, the YAML frontmatter keys, the Starlight asides (`:::note`, `:::tip`, `:::caution`), links and image paths exactly as they are.
- Translate only the text: the frontmatter values (`title`, `description`) included.
- Internal links `/razdor/wiki/<path>` become `/razdor/wiki/en/<path>`.
- Keep the menu and button names meaning-for-meaning, as the game shows them in English; keep Razdor's own names as they are.
- Output only the translated page, with no commentary before or after it."""

INTERNAL_LINK = re.compile(r"/razdor/wiki/(?!en/)")


class OllamaError(Exception):
    pass


def chat(url, model, system, user):
    payload = {
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "temperature": 0.3,
        "stream": False,
        "reasoning_effort": "none",
    }
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT_S) as response:
            body = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", "replace")[:500]
        raise OllamaError(f"Ollama answered HTTP {error.code} for model {model}: {detail}")
    except urllib.error.URLError as error:
        raise OllamaError(
            f"Ollama is unreachable at {url} ({error.reason}). Start Ollama and check the model name."
        )
    except (TimeoutError, OSError) as error:
        raise OllamaError(f"Ollama did not answer within {TIMEOUT_S} s: {error}")
    except json.JSONDecodeError:
        raise OllamaError("Ollama returned something that is not JSON.")
    try:
        text = body["choices"][0]["message"]["content"]
    except (KeyError, IndexError, TypeError):
        raise OllamaError(f"Unexpected reply from Ollama: {json.dumps(body)[:500]}")
    usage = body.get("usage") or {}
    print(
        "tokens: prompt={} completion={} total={}".format(
            usage.get("prompt_tokens", "?"),
            usage.get("completion_tokens", "?"),
            usage.get("total_tokens", "?"),
        ),
        file=sys.stderr,
    )
    return strip_fence(text)


def strip_fence(text):
    """Removes a ``` or ```markdown fence that wraps the whole reply."""
    stripped = text.strip()
    match = re.match(r"^```(?:markdown|md)?[ \t]*\n(.*)\n```$", stripped, re.DOTALL)
    if match:
        return match.group(1).rstrip() + "\n"
    return stripped + "\n"


def write_page(args):
    brief = Path(args.brief).read_text(encoding="utf-8")
    parts = [brief.rstrip(), ""]
    for source in args.source:
        content = Path(source).read_text(encoding="utf-8")
        parts.append(f"=== SOURCE {source} ===")
        parts.append(content.rstrip())
        parts.append(f"=== END SOURCE {source} ===")
        parts.append("")
    page = chat(args.url, args.model, WRITE_SYSTEM, "\n".join(parts))
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(page, encoding="utf-8")
    print(f"wrote {out}", file=sys.stderr)


def translate_page(args):
    source = Path(args.input).read_text(encoding="utf-8")
    page = chat(args.url, args.model, TRANSLATE_SYSTEM, source)
    # Internal links are fixed in code too, whatever the model did with them.
    page = INTERNAL_LINK.sub("/razdor/wiki/en/", page)
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(page, encoding="utf-8")
    print(f"wrote {out}", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--model", default=DEFAULT_MODEL, help="Ollama model (default %(default)s)")
    parser.add_argument("--url", default=DEFAULT_URL, help="chat completions endpoint")
    sub = parser.add_subparsers(dest="mode", required=True)

    write = sub.add_parser("write", help="write a Russian wiki page from a brief and sources")
    write.add_argument("--brief", required=True, help="the brief (Markdown)")
    write.add_argument("--source", action="append", default=[], required=True, help="a source file (repeatable)")
    write.add_argument("--out", required=True, help="the page to write")
    write.set_defaults(run=write_page)

    translate = sub.add_parser("translate", help="translate a Russian wiki page into English")
    translate.add_argument("--in", dest="input", required=True, help="the Russian page")
    translate.add_argument("--out", required=True, help="the English page to write")
    translate.set_defaults(run=translate_page)

    args = parser.parse_args()
    try:
        args.run(args)
    except OllamaError as error:
        print(f"gemma.py: {error}", file=sys.stderr)
        return 1
    except OSError as error:
        print(f"gemma.py: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
