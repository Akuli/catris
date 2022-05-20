import argparse
import asyncio
import logging
import socket

from catris.server_and_client import Server

# The code below is written weirdly make mypy realize that the import might fail.
# If you put reveal_type(websockets_serve) after it, you should see Union[..., None]
websockets_serve = None
try:
    from websockets.server import serve as _a_temporary_variable

    websockets_serve = _a_temporary_variable
except ImportError:
    pass


async def main() -> None:
    logging.basicConfig(level=logging.INFO, format="[%(levelname)s] %(message)s")

    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--lobbies",
        action="store_true",
        help="allow users to create and join lobbies instead of having everyone play together",
    )
    args = parser.parse_args()
    catris_server = Server(args.lobbies)

    tcp_server = await asyncio.start_server(
        catris_server.handle_raw_tcp_connection, port=12345
    )

    # Send TCP keepalive packets periodically as configured in /etc/sysctl.conf
    # Prevents clients from disconnecting after about 5 minutes of inactivity.
    for sock in tcp_server.sockets:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_KEEPALIVE, 1)

    logging.info("Listening for raw TCP connections on port 12345...")

    if websockets_serve is None:
        logging.warning(
            "The web UI won't work because the \"websockets\" module isn't installed."
            " See the README for installation instructions."
        )
        async with tcp_server:
            await tcp_server.serve_forever()
    else:
        # hide unnecessary INFO messages
        logging.getLogger("websockets.server").setLevel(logging.WARNING)

        ws_server = websockets_serve(
            catris_server.handle_websocket_connection,
            port=54321,
            close_timeout=0.1,  # See the code that closes connections for an explanation
        )
        logging.info("Listening for websocket connections on port 54321...")
        async with tcp_server, ws_server:
#
#            import socket as socketmodule
#
#            sockets = []
#            for i in range(5):
#                s = socketmodule.create_connection(('localhost', 12345))
#                s.sendall(b'a%d\rr\raaaaaaaaaas' % i)
#                sockets.append(s)
#            print("Lol...")
            await asyncio.wait_for(tcp_server.serve_forever(), timeout=10)


asyncio.run(main())
