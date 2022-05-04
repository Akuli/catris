# catris

This is a Tetris clone for multiple players that connect to a server with netcat or a web interface.

![Screenshot](screenshot.png)

If you aren't using Windows,
you can play on my server by running this command on a terminal:

```
stty raw; nc 172.104.132.97 12345; stty cooked
```

<details>
<summary>Explanation of what the command does</summary>

The `stty raw` is needed to send key presses to the server
as you press the keys, without first waiting for you to press Enter.
If you forget it, you will get an error message that tells you to use it.

Here `nc`, short for netcat, opens a TCP connection to my server.
It sends its input (your key presses) to the server
and displays what it receives on the terminal.

On some systems, the `stty` and `nc` commands must be ran at once using e.g. `;` as shown above,
instead of entering them separately.

</details>

There's also a web UI, which is useful especially for windows users.
[Click here](http://172.104.132.97) to play.

My server is in Europe, so the game may be very laggy if you're not in Europe.
Please create an issue if this is a problem for you.


## How to play

Before a game starts, you need to make a lobby.
If you want, you can share the lobby ID with your friends
so that they can join the lobby and play with you.

Keys:
- Ctrl+C, Ctrl+D or Ctrl+Q: quit
- Ctrl+R: redraw the whole screen (may be needed after resizing the window)
- WASD or arrow keys: move and rotate
- h: hold (aka "save") block for later, switch to previously held block if any
- r: change rotating direction
- p: pause/unpause (affects all players)
- f: flip the game upside down (only available in ring mode with 1 player)

There's only one score; you play together, not against other players.
Try to collaborate and make the best use of everyone's blocks.

With multiple players, if your playing area
fills up all the way to the top of the game,
you will have to wait 30 seconds.
Then your playing area will be erased and you can continue playing.
The game ends if all players are simultaneously on their 30 seconds waiting time.
This means that you can intentionally fill your area with blocks
to get your waiting time before other players.


## Development

If you're on Windows, use `py` instead of `python3` and `env\Scripts\activate` instead of `source env/bin/activate` below.

```
git clone https://github.com/Akuli/catris
cd catris
python3 -m venv env
source env/bin/activate
pip install -r requirements.txt
pip install -r requirements-dev.txt
python3 -m catris
```

When `python3 -m catris` is running, you can connect to it with netcat:

```
stty raw; nc localhost 12345; stty cooked
```

If you want to develop the web UI, you need to run `python3 -m http.server` in a separate terminal,
and then open `http://localhost:8000/` in your web browser.

Linters and formatters:

```
black catris        # Formats the code
isort catris        # Formats and sorts imports
mypy catris         # Type checker, detects common mistakes
pyflakes catris     # Linter, detects some less common mistakes
```

All these tools also run on GitHub Actions,
so you probably don't need to run them yourself
if you only want to make a couple small changes.
