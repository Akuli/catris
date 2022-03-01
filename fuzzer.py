import random
import socket

commands = [
    b"\x08",
    b"\x7f",
    b"\x1b[A",
    b"\x1b[B",
    b"\x1b[C",
    b"\x1b[D",
    b"\r",
    b"\n",
    b"w",
    b"a",
    b"s",
    b"d",
    b"f",
    b"x",
    b"hello world :)",
]
conn = socket.create_connection(("localhost", 12345))
while True:
    conn.send(random.choice(commands))
