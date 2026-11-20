"""What the phone scripts share: Safari's Web Inspector on an iPhone that is
plugged in, by way of pymobiledevice3.

    PAGE    part of the address of the pages to work on (default: kipukas)
"""

import asyncio
import json
import os

from pymobiledevice3.exceptions import NoDeviceConnectedError
from pymobiledevice3.lockdown import create_using_usbmux
from pymobiledevice3.services.webinspector import WebinspectorService

PAGE = os.environ.get("PAGE", "kipukas")


def run(main) -> int:
    """Run a script's `main`, and say so plainly if no phone answers."""
    try:
        return asyncio.run(main())
    except NoDeviceConnectedError:
        print("No iPhone answers. Plug it in, unlock it, and trust this Mac if it asks.")
        return 2


async def connect() -> WebinspectorService:
    """The inspector of the phone that is plugged in, connected."""
    lockdown = await create_using_usbmux()
    inspector = WebinspectorService(lockdown=lockdown)
    await inspector.connect()
    return inspector


async def our_pages(inspector: WebinspectorService, timeout: float = 3.0) -> list:
    """The pages open on the phone whose address has PAGE in it, in any app."""
    pages = await inspector.get_open_application_pages(timeout=timeout)
    return [p for p in pages if PAGE in (p.page.web_url or "")]


async def attach(inspector: WebinspectorService, target, wait: float = 8.0):
    """A session on one page, ready to evaluate in."""
    session = await asyncio.wait_for(
        inspector.inspector_session(target.application, target.page), wait
    )
    await session.runtime_enable()
    return session


async def evaluate(session, expression: str, wait: float = 5.0):
    """What a small expression comes to in the page, by value.

    Keep it small. One inspector message of more than a few kilobytes is
    held back by the phone: the session answers nothing from then on, though
    the script does run later. `put_in` sends a large script in pieces.
    """
    return await asyncio.wait_for(
        session.runtime_evaluate(expression, return_by_value=True), wait
    )


async def put_in(session, script: str, piece: int = 1200):
    """Run a script of any size in the page, and say what it came to."""
    await evaluate(session, "(window.__kpkSrc = '', 'ok')")
    for at in range(0, len(script), piece):
        await evaluate(session, f"(window.__kpkSrc += {json.dumps(script[at:at + piece])}, 'ok')")
    return await evaluate(
        session,
        "(() => { const src = window.__kpkSrc; delete window.__kpkSrc; return (0, eval)(src); })()",
    )
