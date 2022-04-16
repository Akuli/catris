import argparse
import asyncio
import logging

from catris.server_and_client import Server


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
    asyncio_server = await asyncio.start_server(
        catris_server.handle_connection, port=12345
    )
    async with asyncio_server:
        logging.info("Listening on port 12345...")
        await asyncio_server.serve_forever()


asyncio.run(main())
