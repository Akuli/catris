import asyncio

from catris.server_and_client import Server


async def main() -> None:
    catris_server = Server()
    asyncio_server = await asyncio.start_server(
        catris_server.handle_connection, port=12345
    )
    async with asyncio_server:
        print("Listening on port 12345...")
        await asyncio_server.serve_forever()


asyncio.run(main())
