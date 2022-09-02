# This script is a secure alternative to connecting with netcat.
# Does not work on windows!
#
# For instructions see https://catris.net/ (on a non-Windows computer)
import asyncio
import os
import sys

import aiofiles
import websockets


async def stdin_to_websocket(ws):
    async with aiofiles.open("/dev/stdin", "rb") as file:
        while True:
            await ws.send(await file.read1(1024))


async def main():
    [url] = sys.argv[1:]
    async with websockets.connect(url) as ws:
        asyncio.create_task(stdin_to_websocket(ws))
        try:
            async for message in ws:
                sys.stdout.buffer.write(message)
                sys.stdout.buffer.flush()
        except websockets.exceptions.ConnectionClosed:
            # couldn't get a clean quit to work, asyncio was doing something weird
            os.system("stty cooked")
            os.abort()


os.system("stty raw")
try:
    asyncio.run(main())
finally:
    os.system("stty cooked")
