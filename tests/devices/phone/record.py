"""Record what the QR scanner does on the iPhone that is plugged in, in every
page of the site open on it (Safari, or the home-screen app), for a while.

    uvx --python 3.12 --from pymobiledevice3 python phone/record.py <seconds> <log.jsonl> [commands]

Each page is given instrument.js, again after every page change, and what it
records is added to the log, one JSON object a line, and printed.

`commands` is a folder to watch. A file left in it that ends in .js is run
in the page as one small expression, its answer is recorded, and the file
is removed: a way to press the page's buttons from here while it records.
"""

import asyncio
import json
import logging
import os
import sys
import time

from inspector import attach, connect, evaluate, our_pages, put_in, run

HERE = os.path.dirname(os.path.abspath(__file__))
INSTRUMENT = open(os.path.join(HERE, "instrument.js"), encoding="utf-8").read()
DRAIN = "window.__kpkDiag ? window.__kpkDiag.drain() : 'none'"

logging.getLogger().setLevel(logging.CRITICAL)


async def main() -> int:
    seconds = float(sys.argv[1])
    log = open(sys.argv[2], "a", encoding="utf-8")
    commands = sys.argv[3] if len(sys.argv) > 3 else ""

    def write(app: str, entry: dict) -> None:
        entry = {"mac": time.strftime("%H:%M:%S"), "app": app, **entry}
        log.write(json.dumps(entry, ensure_ascii=False) + "\n")
        log.flush()
        data = entry.get("data")
        said = json.dumps(data, ensure_ascii=False) if data is not None else ""
        print(f"{entry['mac']} [{app}] {entry.get('page', '')} | {entry.get('what')}: {said}", flush=True)

    inspector = await connect()
    sessions: dict = {}  # (app id, page id) -> (session, app name)
    seen_none = 0
    deadline = time.time() + seconds
    try:
        async with inspector:
            while time.time() < deadline:
                try:
                    ours = {(p.application.id_, p.page.id_): p for p in await our_pages(inspector, 1.5)}
                except Exception as error:  # the phone was locked or unplugged for a moment
                    write("-", {"what": "recorder: pages could not be listed", "data": str(error)[:200]})
                    await asyncio.sleep(2)
                    continue
                if not ours:
                    seen_none += 1
                    if seen_none in (1, 20):
                        write("-", {"what": "recorder: no page of the site is open on the phone", "data": None})
                else:
                    seen_none = 0
                for key in [k for k in sessions if k not in ours]:
                    write(sessions[key][1], {"what": "recorder: page closed", "data": None})
                    del sessions[key]
                for key, target in ours.items():
                    if key in sessions:
                        continue
                    name = target.application.name
                    try:
                        sessions[key] = (await attach(inspector, target), name)
                        write(name, {"what": "recorder: attached", "data": target.page.web_url})
                    except Exception as error:
                        write(name, {"what": "recorder: could not attach", "data": type(error).__name__})
                # Take what each page has recorded; put the instrument in if the page is new.
                for _ in range(6):
                    for key, (session, name) in list(sessions.items()):
                        try:
                            out = await evaluate(session, DRAIN)
                            if out == "none":
                                await put_in(session, INSTRUMENT)
                                out = await evaluate(session, DRAIN)
                            if out and out != "none":
                                for entry in json.loads(out):
                                    write(name, entry)
                            for command in sorted(os.listdir(commands)) if commands else []:
                                if not command.endswith(".js"):
                                    continue
                                path = os.path.join(commands, command)
                                expression = open(path, encoding="utf-8").read()
                                os.remove(path)
                                answer = await evaluate(session, expression, 8)
                                write(name, {"what": f"recorder: ran {command}", "data": answer})
                        except Exception as error:
                            said = (type(error).__name__ + " " + str(error))[:160]
                            write(name, {"what": "recorder: lost the page, attaching again", "data": said})
                            del sessions[key]
                    await asyncio.sleep(0.3)
    finally:
        await inspector.close()
        log.close()
    print("recorder: time is up")
    return 0


sys.exit(run(main))
