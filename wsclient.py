# This script is a secure alternative to connecting with netcat.
# Does not work on windows!
#
#   $ python3 -m venv env
#   $ . env/bin/activate
#   $ pip install aiofiles websockets
#   $ python3 wsclient.py
#
import asyncio
import os
import sys

import aiofiles
import websockets


async def stdin_to_websocket(ws):
    async with aiofiles.open("/dev/stdin", "rb") as file:
        while True:
            await ws.send(await file.read1(1024))


async def websocket_to_stdout(ws):
    async for message in ws:
        assert isinstance(message, bytes)
        sys.stdout.buffer.write(message)
        sys.stdout.buffer.flush()


async def hello():
    async with websockets.connect("wss://catris.net/websocket") as ws:
        asyncio.create_task(stdin_to_websocket(ws))
        try:
            await websocket_to_stdout(ws)
        except websockets.exceptions.ConnectionClosed:
            # couldn't get a clean quit to work, asyncio was doing something weird
            os.system("stty cooked")
            os.abort()


os.system("stty raw")
try:
    asyncio.run(hello())
finally:
    os.system("stty cooked")
