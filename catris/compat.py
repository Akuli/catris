import sys

if sys.version_info >= (3, 9):
    from asyncio import to_thread as to_thread
else:
    import asyncio
    from typing import Any

    # copied from source code with slight modifications
    async def to_thread(func: Any, *args: Any, **kwargs: Any) -> Any:
        import contextvars
        import functools

        loop = asyncio.get_running_loop()
        ctx = contextvars.copy_context()
        func_call = functools.partial(ctx.run, func, *args, **kwargs)
        return await loop.run_in_executor(None, func_call)
