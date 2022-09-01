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


# TODO: Do this in catris, so that the code here would be shorter
async def send_full_characters_and_ansi_codes(send_queue, ws):
    while send_queue:
        count = 0
        if send_queue[0] == 0x1B:
            # ansi code, it will end with some uppercase letter
            for index, char in send_queue:
                if char in b"ABCDEFGHIJKLMNOPQRSTUVWXYZ":
                    count = index + 1
                    break
            else:
                # part of ansi code still missing
                return
        elif send_queue[0] < 128:
            # ascii character
            count = 1
        elif send_queue[0] >> 5 == 0b110:
            # utf-8 two-byte character
            count = 2
        elif send_queue[0] >> 4 == 0b1110:
            # utf-8 three-byte character
            count = 3
        elif send_queue[0] >> 3 == 0b11110:
            # utf-8 four-byte character
            count = 4
        else:
            # invalid input
            send_queue = send_queue[1:]
            break

        if len(send_queue) < count:
            return

        await ws.send(send_queue[:count])
        del send_queue[:count]


async def stdin_to_websocket(ws):
    async with aiofiles.open("/dev/stdin", "rb") as file:
        send_queue = bytearray()
        while True:
            send_queue += await file.read1(1024)
            await send_full_characters_and_ansi_codes(send_queue, ws)


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
