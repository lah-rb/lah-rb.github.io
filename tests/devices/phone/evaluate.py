"""Run a script in a page open in Safari on the iPhone that is plugged in,
and print what it returns.

    uvx --python 3.12 --from pymobiledevice3 python phone/evaluate.py <file.js> [seconds] [page]

The file is the body of an async function: it may use await, and what it
returns is printed as JSON. `seconds` is how long to wait for it (15).
`page` picks among several open pages by part of the address; without it
the first is used, and all of them are listed.
"""

import asyncio
import json
import sys
import time

from inspector import attach, connect, evaluate, our_pages, put_in, run


async def main() -> int:
    body = open(sys.argv[1], encoding="utf-8").read()
    wait = float(sys.argv[2]) if len(sys.argv) > 2 else 15.0
    which = sys.argv[3] if len(sys.argv) > 3 else ""
    inspector = await connect()
    try:
        async with inspector:
            pages = await our_pages(inspector)
            if not pages:
                print("NO PAGE: no such page is open on the phone")
                return 2
            for page in pages:
                print(f"open: {page.application.name}: {page.page.web_url}", file=sys.stderr)
            chosen = [p for p in pages if which in p.page.web_url]
            if not chosen:
                print(f"NO PAGE: none of them has {which!r} in its address")
                return 2
            session = await attach(inspector, chosen[0])
            # The page puts its answer in a global, which is asked for until
            # it is there: the inspector does not wait for a promise.
            await put_in(
                session,
                "(() => { window.__kpkOut = null; (async () => {" + body + "\n})().then("
                "(v) => { window.__kpkOut = JSON.stringify({ ok: v === undefined ? null : v }); },"
                "(e) => { window.__kpkOut = JSON.stringify({ error: String((e && e.stack) || e) }); });"
                " return 'started'; })()",
            )
            deadline = time.time() + wait
            while time.time() < deadline:
                out = await evaluate(session, "window.__kpkOut || ''")
                if out:
                    result = json.loads(out)
                    print(json.dumps(result.get("ok", result), indent=2, ensure_ascii=False))
                    return 0 if "ok" in result else 1
                await asyncio.sleep(0.2)
            print("TIMED OUT waiting for the page to answer")
            return 3
    finally:
        await inspector.close()


sys.exit(run(main))
