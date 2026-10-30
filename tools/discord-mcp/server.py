#!/usr/bin/env python3
"""MCP server for Razdor's player reports on Discord.

Speaks MCP (JSON-RPC 2.0, one message per line) on stdin/stdout, standard library only.
It reads the reports channel or thread with a bot token. The read-only tools (fetch_recent,
get_message, download_attachments) can also read any other channel or thread the bot sees, by a
`channel` argument or a message link. Every Discord call is a GET except the writes of
`reports_post`: a POST for its message (with up to 10 attached images) and a PUT per reaction
(up to 5). Writes go only to the reports channel, and only when DISCORD_ALLOW_POST=1 and the call passes
confirm=true (without it, it returns a preview). Its mentions are limited
to users recorded as reporters; @everyone, @here and roles never ping. Locally it writes
downloaded attachments and a state file: the cursor and which Discord users reported which
GitHub issue (kept off GitHub, so issues carry no Discord ids).

Settings come from the environment or from the repo's `.env` (KEY=VALUE, quotes allowed):

  DISCORD_BOT_TOKEN        the bot's token (Developer Portal → Bot → Token)
  DISCORD_REPORTS_CHANNEL  the channel or thread to read: its id, or a link
                           https://discord.com/channels/<guild>/<channel>[/<message>]
  DISCORD_DOWNLOAD_DIR     where attachments go (default ~/.cache/razdor-discord/files)
  DISCORD_STATE_FILE       the cursor (default ~/.cache/razdor-discord/state.json)
  DISCORD_ALLOW_POST       1 to let reports_post send (the bot then needs Send Messages)

Attachments are untrusted files from strangers: each message gets its own folder, names are
reduced to safe characters, and nothing downloaded is ever opened or run by this server.

Offline checks (multipart body, file and reaction limits, the 204 path) run with
`python3 -I tools/discord-mcp/server.py --selftest`; they never call Discord.
"""

import json
import os
import re
import secrets
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

API = "https://discord.com/api/v10"
SERVER = {"name": "razdor-discord-reports", "version": "0.1.0"}
MAX_ATTACHMENT = 100 * 1024 * 1024  # bytes; larger files are listed, not fetched
MAX_UPLOAD = 10 * 1024 * 1024  # bytes per image reports_post attaches
MAX_UPLOAD_TOTAL = 25 * 1024 * 1024  # bytes for all images of one message
MAX_FILES = 10
MAX_REACTIONS = 5
REACTION_PAUSE = 0.3  # seconds between reaction calls (rate limits)
IMAGE_TYPES = {".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg",
               ".webp": "image/webp", ".gif": "image/gif"}
REPO = Path(__file__).resolve().parents[2]


def load_env():
    env = {}
    path = REPO / ".env"
    if path.is_file():
        for line in path.read_text(encoding="utf-8").splitlines():
            m = re.match(r"\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$", line)
            if not m or line.lstrip().startswith("#"):
                continue
            value = m.group(2).strip()
            if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
                value = value[1:-1]
            env[m.group(1)] = value
    env.update({k: v for k, v in os.environ.items() if k.startswith("DISCORD_")})
    return env


ENV = load_env()
CACHE = Path.home() / ".cache" / "razdor-discord"
DOWNLOADS = Path(ENV.get("DISCORD_DOWNLOAD_DIR") or CACHE / "files").expanduser()
STATE = Path(ENV.get("DISCORD_STATE_FILE") or CACHE / "state.json").expanduser()


class Fail(Exception):
    pass


CHANNEL_RE = re.compile(r"channels/(\d+|@me)/(\d+)")  # group 2: the channel id
MESSAGE_RE = re.compile(r"channels/(\d+|@me)/(\d+)/(\d+)")  # group 2: channel, group 3: message


def channel_id():
    raw = (ENV.get("DISCORD_REPORTS_CHANNEL") or "").strip()
    m = CHANNEL_RE.search(raw)
    if m:
        return m.group(2)
    if raw.isdigit():
        return raw
    raise Fail("DISCORD_REPORTS_CHANNEL is not set (an id or a discord.com/channels/... link)")


def channel_of(raw):
    """A channel id, or a link to a channel (a message link's message part is ignored)."""
    raw = str(raw).strip()
    m = CHANNEL_RE.search(raw)
    if m:
        return m.group(2)
    if raw.isdigit():
        return raw
    raise Fail("channel must be a channel id or a discord.com/channels/... link")


def parse_ref(message_id=None, channel=None):
    """(channel id, message id or None). A message link sets both and ignores `channel`."""
    raw = str(message_id or "").strip()
    m = MESSAGE_RE.search(raw)
    if m:
        return m.group(2), m.group(3)
    cid = channel_of(channel) if channel and str(channel).strip() else channel_id()
    if raw and not raw.isdigit():
        raise Fail("message_id must be a number")
    return cid, raw or None


def message_part(raw, name):
    """A message id from an id or a message link (None when empty)."""
    raw = str(raw or "").strip()
    if not raw:
        return None
    m = MESSAGE_RE.search(raw)
    raw = m.group(3) if m else raw
    if not raw.isdigit():
        raise Fail(f"{name} must be a message id or a message link")
    return raw


def request(method, path, data=None, ctype=None, params=None):
    """One Discord call: retries 429 (honouring retry_after), returns the JSON body or None for an empty reply (204)."""
    token = ENV.get("DISCORD_BOT_TOKEN")
    if not token:
        raise Fail("DISCORD_BOT_TOKEN is not set")
    url = API + path + ("?" + urllib.parse.urlencode(params) if params else "")
    headers = {
        "Authorization": "Bot " + token,
        "User-Agent": "DiscordBot (https://github.com/indicozy/razdor, 0.1) razdor-reports",
    }
    if ctype:
        headers["Content-Type"] = ctype
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    for _ in range(3):
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                raw = r.read()
                return json.loads(raw) if raw else None
        except urllib.error.HTTPError as e:
            if e.code == 429:
                try:
                    wait = float(json.load(e).get("retry_after", 1))
                except Exception:
                    wait = 1.0
                time.sleep(min(wait, 10))
                continue
            hint = {401: "bad token", 403: "the bot can't see this channel (permissions, or the "
                    "Message Content intent is off)", 404: "no such channel or message"}.get(e.code, "")
            try:
                detail = json.load(e).get("message", "")
            except Exception:
                detail = ""
            msg = f"Discord answered {e.code} for {method} {path}"
            msg += f" ({hint})" if hint else ""
            msg += f": {detail}" if detail else ""
            raise Fail(msg)
        except urllib.error.URLError as e:
            raise Fail(f"network error: {e.reason}")
    raise Fail("Discord kept rate-limiting; try again later")


def get(path, params=None, body=None):
    if body is None:
        return request("GET", path, params=params)
    return request("POST", path, data=json.dumps(body).encode(), ctype="application/json", params=params)


_guild = {}


def guild_of(cid):
    if cid not in _guild:
        _guild[cid] = get(f"/channels/{cid}").get("guild_id") or "@me"
    return _guild[cid]


def shape(m, cid):
    author = m.get("author") or {}
    ref = m.get("message_reference") or {}
    return {
        "id": m["id"],
        "link": f"https://discord.com/channels/{guild_of(cid)}/{cid}/{m['id']}",
        "time": m.get("timestamp"),
        "edited": m.get("edited_timestamp"),
        "author": author.get("global_name") or author.get("username"),
        "author_id": author.get("id"),
        "is_bot": bool(author.get("bot")),
        "content": m.get("content", ""),
        "reply_to": ref.get("message_id"),
        "attachments": [{"name": a.get("filename"), "size": a.get("size"),
                         "type": a.get("content_type")} for a in m.get("attachments", [])],
        "embeds": [e.get("url") or e.get("title") for e in m.get("embeds", []) if e.get("url") or e.get("title")],
    }


def read_state():
    try:
        return json.loads(STATE.read_text())
    except Exception:
        return {}


def write_state(state):
    STATE.parent.mkdir(parents=True, exist_ok=True)
    tmp = STATE.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, indent=1))
    tmp.replace(STATE)


def fetch_new(limit=50):
    cid = channel_id()
    after = read_state().get(cid)
    limit = max(1, min(int(limit), 500))
    out = []
    if after is None:
        # No cursor yet: the newest `limit` messages, oldest first.
        msgs, before = [], None
        while len(msgs) < limit:
            page = get(f"/channels/{cid}/messages", {"limit": min(100, limit - len(msgs)), **({"before": before} if before else {})})
            if not page:
                break
            msgs += page
            before = page[-1]["id"]
        out = list(reversed(msgs))
    else:
        while len(out) < limit:
            page = get(f"/channels/{cid}/messages", {"limit": min(100, limit - len(out)), "after": after})
            if not page:
                break
            page.sort(key=lambda m: int(m["id"]))
            out += page
            after = page[-1]["id"]
    return {"channel": cid, "cursor": read_state().get(cid), "count": len(out),
            "messages": [shape(m, cid) for m in out],
            "note": "Call reports_mark_read with the last id handled to move the cursor."}


def fetch_recent(limit=20, channel=None, before=None, after=None):
    cid, _ = parse_ref(None, channel)
    b, a = message_part(before, "before"), message_part(after, "after")
    params = {"limit": max(1, min(int(limit), 100)), **({"before": b} if b else {}), **({"after": a} if a else {})}
    page = sorted(get(f"/channels/{cid}/messages", params), key=lambda m: int(m["id"]))
    return {"channel": cid, "messages": [shape(m, cid) for m in page]}


def get_message(message_id, channel=None):
    cid, mid = parse_ref(message_id, channel)
    if not mid:
        raise Fail("message_id must be a number")
    return shape(get(f"/channels/{cid}/messages/{mid}"), cid)


def safe_name(name):
    name = re.sub(r"[^A-Za-z0-9._-]+", "_", name or "file").strip("._") or "file"
    return name[:120]


def download(message_id, channel=None):
    cid, mid = parse_ref(message_id, channel)
    if not mid:
        raise Fail("message_id must be a number")
    m = get(f"/channels/{cid}/messages/{mid}")
    folder = DOWNLOADS / mid
    folder.mkdir(parents=True, exist_ok=True)
    files = []
    for i, a in enumerate(m.get("attachments", [])):
        name = f"{i:02d}-{safe_name(a.get('filename'))}"
        entry = {"name": a.get("filename"), "size": a.get("size"), "type": a.get("content_type")}
        url = a.get("url", "")
        host = urllib.parse.urlparse(url).hostname or ""
        if not (host == "cdn.discordapp.com" or host == "media.discordapp.net"):
            entry["skipped"] = "not a Discord CDN url"
        elif (a.get("size") or 0) > MAX_ATTACHMENT:
            entry["skipped"] = "larger than the limit"
        else:
            path = folder / name
            req = urllib.request.Request(url, headers={"User-Agent": "razdor-reports"})
            with urllib.request.urlopen(req, timeout=120) as r, open(path, "wb") as f:
                got = 0
                while chunk := r.read(1 << 16):
                    got += len(chunk)
                    if got > MAX_ATTACHMENT:
                        raise Fail(f"{name}: larger than the limit")
                    f.write(chunk)
            entry["path"] = str(path)
        files.append(entry)
    return {"message": shape(m, cid), "folder": str(folder), "files": files,
            "note": "Untrusted files: look at them, don't run them."}


def mark_read(message_id):
    cid = channel_id()
    if not str(message_id).isdigit():
        raise Fail("message_id must be a number")
    state = read_state()
    old = state.get(cid)
    if old is None or int(message_id) > int(old):
        state[cid] = str(message_id)
        write_state(state)
    return {"channel": cid, "cursor": state.get(cid), "previous": old}


def link_issue(issue, message_ids):
    cid = channel_id()
    issue = str(int(issue))
    state = read_state()
    reporters = state.setdefault("reporters", {})
    users = state.setdefault("users", {})
    ids = reporters.setdefault(issue, [])
    for mid in message_ids:
        if not str(mid).isdigit():
            raise Fail(f"{mid}: message ids are numbers")
        m = get(f"/channels/{cid}/messages/{mid}")
        a = m.get("author") or {}
        if a.get("bot") or not a.get("id"):
            continue
        users[a["id"]] = a.get("global_name") or a.get("username")
        if a["id"] not in ids:
            ids.append(a["id"])
    write_state(state)
    return {"issue": issue, "reporters": [{"id": u, "name": users.get(u)} for u in ids]}


def reporters_of(issues):
    state = read_state()
    users = state.get("users", {})
    out = {}
    for i in issues:
        ids = state.get("reporters", {}).get(str(int(i)), [])
        out[str(int(i))] = [{"id": u, "name": users.get(u), "mention": f"<@{u}>"} for u in ids]
    return out


def chunks(text, size=1900):
    parts, cur = [], ""
    for line in text.split("\n"):
        while len(line) > size:
            if cur:
                parts.append(cur)
                cur = ""
            parts.append(line[:size])
            line = line[size:]
        if len(cur) + len(line) + 1 > size:
            parts.append(cur)
            cur = line
        else:
            cur = f"{cur}\n{line}" if cur else line
    if cur:
        parts.append(cur)
    return parts


def check_files(paths):
    """Validate the local images for reports_post; returns their Paths (nothing is read yet)."""
    if paths is None:
        return []
    if not isinstance(paths, list):
        raise Fail("files must be an array of local file paths")
    if len(paths) > MAX_FILES:
        raise Fail(f"at most {MAX_FILES} files per message")
    out, total = [], 0
    for raw in paths:
        if not isinstance(raw, str) or not raw.strip():
            raise Fail("each file must be a non-empty path")
        path = Path(raw.strip()).expanduser()
        if path.suffix.lower() not in IMAGE_TYPES:
            raise Fail(f"{path.name}: only .png, .jpg, .jpeg, .webp or .gif can be attached")
        if not path.is_file():
            raise Fail(f"{path.name}: no such file")
        size = path.stat().st_size
        if size > MAX_UPLOAD:
            raise Fail(f"{path.name}: {size} bytes; the limit is 10 MB per file")
        total += size
        out.append(path)
    if total > MAX_UPLOAD_TOTAL:
        raise Fail("the files add up to more than 25 MB")
    return out


def check_reactions(emojis):
    if emojis is None:
        return []
    if not isinstance(emojis, list):
        raise Fail("reactions must be an array of emoji strings")
    if len(emojis) > MAX_REACTIONS:
        raise Fail(f"at most {MAX_REACTIONS} reactions per message")
    out = []
    for e in emojis:
        if not isinstance(e, str) or not e.strip():
            raise Fail("each reaction must be a non-empty emoji string")
        out.append(e.strip())
    return out


def multipart(payload, files):
    """multipart/form-data body: payload_json plus files[i] parts. files = [(name, bytes, content_type)]."""
    boundary = "razdor" + secrets.token_hex(16)
    out = [f'--{boundary}\r\nContent-Disposition: form-data; name="payload_json"\r\n'
           "Content-Type: application/json\r\n\r\n".encode(),
           json.dumps(payload, ensure_ascii=False).encode("utf-8"), b"\r\n"]
    for i, (name, data, ctype) in enumerate(files):
        safe = re.sub(r"[\x00-\x1f]", "_", name).replace("\\", "\\\\").replace('"', '\\"')
        out.append(f'--{boundary}\r\nContent-Disposition: form-data; name="files[{i}]"; filename="{safe}"\r\n'
                   f"Content-Type: {ctype}\r\n\r\n".encode("utf-8"))
        out += [data, b"\r\n"]
    out.append(f"--{boundary}--\r\n".encode())
    return b"".join(out), f"multipart/form-data; boundary={boundary}"


def send_with_files(cid, body, paths):
    """Post one message carrying the images (their bytes are read here, after all checks)."""
    blobs = []
    for p in paths:
        data = p.read_bytes()
        if len(data) > MAX_UPLOAD:
            raise Fail(f"{p.name}: changed while sending; it is over 10 MB now")
        blobs.append((p.name, data, IMAGE_TYPES[p.suffix.lower()]))
    payload = {**body, "attachments": [{"id": i, "filename": p.name} for i, p in enumerate(paths)]}
    data, ctype = multipart(payload, blobs)
    return request("POST", f"/channels/{cid}/messages", data=data, ctype=ctype)


def post(text, reply_to=None, confirm=False, files=None, reactions=None):
    cid = channel_id()
    text = (text or "").strip()
    if not text:
        raise Fail("empty message")
    paths = check_files(files)
    emojis = check_reactions(reactions)
    known = {u for ids in read_state().get("reporters", {}).values() for u in ids}
    mentioned = re.findall(r"<@!?(\d+)>", text)
    pinged = sorted({u for u in mentioned if u in known})
    unknown = sorted({u for u in mentioned if u not in known})
    parts = chunks(text)
    preview = {"channel": cid, "parts": parts, "pings": pinged,
               "not_pinged": unknown, "reply_to": reply_to,
               "files": [{"name": p.name, "size": p.stat().st_size} for p in paths],
               "reactions": emojis}
    if not confirm:
        preview["note"] = "Preview only. Show it to the owner; call again with confirm=true after they approve."
        return preview
    if ENV.get("DISCORD_ALLOW_POST") != "1":
        raise Fail("posting is off: set DISCORD_ALLOW_POST=1 in .env (and give the bot Send Messages)")
    if reply_to is not None and not str(reply_to).isdigit():
        raise Fail("reply_to must be a message id")
    sent = []
    for i, part in enumerate(parts):
        body = {"content": part, "allowed_mentions": {"parse": [], "users": pinged[:100]}}
        if i == 0 and reply_to:
            body["message_reference"] = {"message_id": str(reply_to), "fail_if_not_exists": False}
        if paths and i == len(parts) - 1:  # the images go on the last part's message
            m = send_with_files(cid, body, paths)
        else:
            m = get(f"/channels/{cid}/messages", body=body)
        sent.append({"id": m["id"], "link": f"https://discord.com/channels/{guild_of(cid)}/{cid}/{m['id']}"})
    added = []
    last = sent[-1]
    for n, emoji in enumerate(emojis):  # reactions go on the last message
        if n:
            time.sleep(REACTION_PAUSE)
        try:
            request("PUT", f"/channels/{cid}/messages/{last['id']}/reactions/{urllib.parse.quote(emoji, safe='')}/@me",
                    data=b"")
        except Fail as e:
            raise Fail(f"{e}; the message is posted at {last['link']}, reactions added: {added or 'none'}")
        added.append(emoji)
    return {"sent": sent, "pings": pinged, "files": [p.name for p in paths], "reactions": added}


def status():
    state = read_state()
    out = {"token_set": bool(ENV.get("DISCORD_BOT_TOKEN")), "posting": ENV.get("DISCORD_ALLOW_POST") == "1", "downloads": str(DOWNLOADS), "state_file": str(STATE)}
    try:
        me = get("/users/@me")
        out["bot"] = me.get("username")
    except Fail as e:
        out["token_error"] = str(e)
    try:
        cid = channel_id()
        ch = get(f"/channels/{cid}")
        out.update({"channel": cid, "name": ch.get("name"), "guild": ch.get("guild_id"), "cursor": state.get(cid)})
    except Fail as e:
        out["error"] = str(e)
    return out


ID = {"type": "string", "description": "A Discord message id (digits), or a message link (its channel is then used)."}
CHANNEL = {"type": "string", "description": "Another channel or thread to read (its id or a discord.com/channels/... link); "
           "default the reports channel. Read-only."}
TOOLS = [
    ("reports_status", "Check the setup: token present, which channel/thread is read, the cursor.",
     {}, lambda a: status()),
    ("reports_fetch_new", "Messages in the reports channel after the cursor (the last message marked read), oldest first. "
     "Without a cursor: the newest `limit` messages. Read-only; does not move the cursor.",
     {"limit": {"type": "integer", "description": "At most this many (1-500, default 50)."}},
     lambda a: fetch_new(a.get("limit", 50))),
    ("reports_fetch_recent", "The newest messages regardless of the cursor, oldest first. Reads the reports channel, or "
     "`channel` (any channel or thread the bot can see); `before`/`after` page through it.",
     {"limit": {"type": "integer", "description": "1-100, default 20."}, "channel": CHANNEL,
      "before": {"type": "string", "description": "Message id or link: only messages before it."},
      "after": {"type": "string", "description": "Message id or link: only messages after it."}},
     lambda a: fetch_recent(a.get("limit", 20), a.get("channel"), a.get("before"), a.get("after"))),
    ("reports_get_message", "One message by id or link (e.g. the one a report replies to), in the reports channel or any "
     "channel/thread the bot can see.",
     {"message_id": ID, "channel": CHANNEL}, lambda a: get_message(a["message_id"], a.get("channel"))),
    ("reports_download_attachments", "Download a message's attachments (screenshots, videos, saves) to a folder of their own "
     "and return the paths. Fetches fresh links, so it works after the old CDN links expired. By id or link, in the reports "
     "channel or any channel/thread the bot can see.",
     {"message_id": ID, "channel": CHANNEL}, lambda a: download(a["message_id"], a.get("channel"))),
    ("reports_link_issue", "Record (locally) that these messages' authors reported GitHub issue N, for crediting them "
     "when it ships. Call it when filing or commenting on an issue.",
     {"issue": {"type": "integer"}, "message_ids": {"type": "array", "items": ID}},
     lambda a: link_issue(a["issue"], a["message_ids"])),
    ("reports_reporters", "The Discord users recorded for these issues, with their <@id> mention text.",
     {"issues": {"type": "array", "items": {"type": "integer"}}},
     lambda a: reporters_of(a["issues"])),
    ("reports_post", "Post a message to the reports channel (split at 2000 chars). `files` attaches up to 10 local images "
     "(.png/.jpg/.jpeg/.webp/.gif, 10 MB each, 25 MB in all) to the last part; `reactions` adds up to 5 emoji to that "
     "message. Without confirm=true it only returns a preview: show that to the owner first. Pings only recorded "
     "reporters; never @everyone/@here/roles. Needs DISCORD_ALLOW_POST=1.",
     {"text": {"type": "string"}, "reply_to": ID, "confirm": {"type": "boolean"},
      "files": {"type": "array", "items": {"type": "string"},
                "description": "Local image paths (at most 10)."},
      "reactions": {"type": "array", "items": {"type": "string"},
                    "description": "Unicode emoji, e.g. [\"👍\", \"👎\"] (at most 5)."}},
     lambda a: post(a["text"], a.get("reply_to"), bool(a.get("confirm")), a.get("files"), a.get("reactions"))),
    ("reports_mark_read", "Move the local cursor forward to this message once it is handled. Local only: nothing is written to Discord.",
     {"message_id": ID}, lambda a: mark_read(a["message_id"])),
]


def tool_list():
    return [{"name": n, "description": d,
             "inputSchema": {"type": "object", "properties": p,
                             "required": [k for k in p if k in ("message_id", "issue", "message_ids", "issues", "text")]}}
            for n, d, p, _ in TOOLS]


def handle(req):
    # `.env` is read again for every request, so a token or channel set while the server
    # runs is used at once (the paths stay as they were at start).
    ENV.clear()
    ENV.update(load_env())
    method, params = req.get("method"), req.get("params") or {}
    if method == "initialize":
        return {"protocolVersion": params.get("protocolVersion", "2025-06-18"),
                "capabilities": {"tools": {}}, "serverInfo": SERVER}
    if method == "ping":
        return {}
    if method == "tools/list":
        return {"tools": tool_list()}
    if method == "tools/call":
        fn = {n: f for n, _, _, f in TOOLS}.get(params.get("name"))
        if fn is None:
            raise KeyError(params.get("name"))
        try:
            text, error = json.dumps(fn(params.get("arguments") or {}), ensure_ascii=False, indent=1), False
        except Fail as e:
            text, error = str(e), True
        except Exception as e:  # report, don't die
            text, error = f"{type(e).__name__}: {e}", True
        return {"content": [{"type": "text", "text": text}], "isError": error}
    raise LookupError(method)


def selftest():
    """Offline checks: no Discord call (urlopen is replaced by a stub for the request path)."""
    import tempfile
    import urllib.request as ur
    from urllib.parse import quote

    def ok(cond, what):
        if not cond:
            raise AssertionError(what)
        print("ok -", what)

    def refused(fn, what):
        try:
            fn()
        except Fail as e:
            print("ok -", what, f"({e})")
            return
        raise AssertionError(what + ": not refused")

    png = b"\x89PNG\r\n\x1a\n" + b"\x00" * 24 + b"fake-picture-data"
    body, ctype = multipart({"content": "hi", "attachments": [{"id": 0, "filename": "a.png"}]},
                            [("a.png", png, "image/png")])
    boundary = ctype.split("boundary=")[1]
    delim = b"--" + boundary.encode()
    ok(ctype == f"multipart/form-data; boundary={boundary}", "content type carries the boundary")
    ok(body.startswith(delim + b"\r\n") and body.endswith(delim + b"--\r\n"), "body opens and closes with the boundary")
    parts = body.split(delim)
    ok(len(parts) == 4 and parts[0] == b"" and parts[3] == b"--\r\n", "one payload part and one file part")
    head1, _, rest1 = parts[1].partition(b"\r\n\r\n")
    ok(b'name="payload_json"' in head1 and b"Content-Type: application/json" in head1, "payload_json part headers")
    ok(json.loads(rest1.rstrip(b"\r\n"))["content"] == "hi", "payload_json carries the content")
    head2, _, rest2 = parts[2].partition(b"\r\n\r\n")
    ok(b'name="files[0]"; filename="a.png"' in head2 and b"Content-Type: image/png" in head2, "file part headers")
    ok(rest2 == png + b"\r\n", "file bytes arrive intact")
    ok(quote("👍", safe="") == "%F0%9F%91%8D", "emoji is percent-encoded for the reaction path")

    with tempfile.TemporaryDirectory() as d:
        d = Path(d)
        good = d / "shot.PNG"
        good.write_bytes(png)
        txt = d / "note.txt"
        txt.write_text("x")
        ok([p.name for p in check_files([str(good)])] == ["shot.PNG"], "upper-case .PNG accepted")
        refused(lambda: check_files([str(txt)]), "bad extension refused")
        refused(lambda: check_files([str(good)] * 11), "eleven files refused")
        refused(lambda: check_files([str(d / "missing.png")]), "missing file refused")
        big = d / "big.png"
        big.write_bytes(b"")
        os.truncate(big, MAX_UPLOAD + 1)
        refused(lambda: check_files([str(big)]), "file over 10 MB refused")
        three = []
        for i in range(3):
            f = d / f"s{i}.gif"
            f.write_bytes(b"GIF89a")
            os.truncate(f, 9 * 1024 * 1024)
            three.append(str(f))
        refused(lambda: check_files(three), "27 MB in total refused")
        refused(lambda: check_reactions(["👍"] * 6), "six reactions refused")
        refused(lambda: check_reactions([""]), "empty reaction refused")
        ok(check_reactions(["👍", "👎"]) == ["👍", "👎"], "two reactions accepted")

    # The request path with a stubbed urlopen: a 204 with no body returns None, and the PUT and multipart POST
    # carry the right method and headers.
    class Reply:
        def __init__(self, raw):
            self.raw = raw

        def read(self):
            return self.raw

        def __enter__(self):
            return self

        def __exit__(self, *exc):
            return False

    seen = []

    def fake_urlopen(req, timeout=None):
        seen.append(req)
        return Reply(b"")

    real_urlopen, had = ur.urlopen, "DISCORD_BOT_TOKEN" in ENV
    old_token = ENV.get("DISCORD_BOT_TOKEN")
    ur.urlopen = fake_urlopen
    ENV["DISCORD_BOT_TOKEN"] = "selftest-token"
    try:
        out = request("PUT", "/channels/1/messages/2/reactions/x/@me", data=b"")
        ok(out is None and seen[-1].get_method() == "PUT", "204 with no body returns None")
        request("POST", "/channels/1/messages", data=body, ctype=ctype)
        ok(seen[-1].get_method() == "POST" and seen[-1].get_header("Content-type") == ctype,
           "multipart POST sends the boundary content type")
    finally:
        ur.urlopen = real_urlopen
        if had:
            ENV["DISCORD_BOT_TOKEN"] = old_token
        else:
            ENV.pop("DISCORD_BOT_TOKEN", None)

    # References: no Discord call (channel_id() reads the reports channel set here).
    had_channel, old_channel = "DISCORD_REPORTS_CHANNEL" in ENV, ENV.get("DISCORD_REPORTS_CHANNEL")
    ENV["DISCORD_REPORTS_CHANNEL"] = "111"
    try:
        ok(parse_ref("https://discord.com/channels/9/222/333") == ("222", "333"), "a message link gives its channel and message")
        ok(parse_ref("333", "https://discord.com/channels/9/444") == ("444", "333"), "digits with a channel link")
        ok(parse_ref("https://discord.com/channels/@me/555/666", "777") == ("555", "666"), "an @me link works and beats channel")
        ok(parse_ref() == ("111", None), "no arguments: the reports channel")
        ok(message_part("https://discord.com/channels/9/1/2", "before") == "2", "before: a link's message part")
        refused(lambda: parse_ref("abc"), "a non-number message refused")
        refused(lambda: parse_ref("1", "nope"), "a bad channel refused")
        refused(lambda: message_part("abc", "after"), "a bad after refused")
    finally:
        if had_channel:
            ENV["DISCORD_REPORTS_CHANNEL"] = old_channel
        else:
            ENV.pop("DISCORD_REPORTS_CHANNEL", None)
    print("selftest passed")


def main():
    if "--selftest" in sys.argv:
        return selftest()
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            continue
        if "id" not in req:
            continue  # a notification
        try:
            resp = {"jsonrpc": "2.0", "id": req["id"], "result": handle(req)}
        except LookupError as e:
            resp = {"jsonrpc": "2.0", "id": req["id"], "error": {"code": -32601, "message": f"unknown: {e}"}}
        except Exception as e:
            resp = {"jsonrpc": "2.0", "id": req["id"], "error": {"code": -32603, "message": str(e)}}
        sys.stdout.write(json.dumps(resp, ensure_ascii=False) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
