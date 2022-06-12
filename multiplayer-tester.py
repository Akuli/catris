# quick and dirty python script for testing changes locally:
#   - connects 3 clients to catris simultaneously
#   - joins all clients to the same lobby and same game
#   - lets you play as one of the clients
#
# does NOT work on windows, let me know if you want to develop this on windows
import os
import select
import sys
import socket
import re

INTERACTIVE_CLIENT_INDEX = 1  # which client will you control on terminal?
GAME_MODE = "t"  # first letter, e.g. r = ring game, t = traditional

sockets = []
received = []


def add_client(to_send):
    sock = socket.socket()
    sock.connect(("localhost", 12345))
    sock.sendall(to_send)
    data = b""
    while not re.search(b"Nothing.*in.*hold.*press.*h", data):
        b = sock.recv(50)
        assert b
        data += b

    sockets.append(sock)
    received.append(data)


# n = new lobby
add_client(f"first\rn\r{GAME_MODE}\r".encode("ascii"))
lobby_id = re.search(rb"Lobby.*ID.*?([A-Z0-9]{6})\b", received[0]).group(1).decode("ascii")
print("got lobby ID:", lobby_id)
# j = join existing lobby
add_client(f"second\rj\r{lobby_id}\r{GAME_MODE}\r".encode("ascii"))
add_client(f"third\rj\r{lobby_id}\r{GAME_MODE}\r".encode("ascii"))

os.system("stty raw")
try:
    sock = sockets[INTERACTIVE_CLIENT_INDEX]
    sys.stdout.buffer.write(received[INTERACTIVE_CLIENT_INDEX])
    sendbuf = bytearray()
    while True:
        can_read, can_write, _ = select.select(
            [sys.stdin.buffer, sock], [sock] if sendbuf else [], []
        )
        if sys.stdin.buffer in can_read:
            sendbuf += sys.stdin.buffer.read1(100)
            if b"\x03" in sendbuf:
                raise KeyboardInterrupt
        if sock in can_read:
            data = sock.recv(1024)
            if not data:
                break
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()
        if sock in can_write:
            n = sock.send(bytes(sendbuf))
            del sendbuf[:n]
finally:
    os.system("stty cooked")
    print("\x1b[?25h")  # show cursor
