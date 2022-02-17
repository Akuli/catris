# catris

This is a Tetris clone for multiple players that connect to a server with netcat.

![Screenshot](screenshot.png)

First, download `catris.py`. It has no dependencies except Python itself.
If you already have Git installed, you can use it:

```
git clone https://github.com/Akuli/catris
cd catris
```

Then run the server (if you're on Windows, use `py` instead of `python3`):

```
python3 catris.py
```

To connect to the server, open a new terminal and run:

```
stty raw && nc localhost 12345
```

Or if you're on Windows, google how to install telnet on whatever windows version you have
(e.g. googling "windows 7 install telnet" worked for me),
and use it instead of `nc`:

```
telnet localhost 12345
```

The port is literally `12345`.
If the server is running on a different computer,
replace `localhost` with the server's IP or hostname.

If you forget `stty raw`, you will get an error message reminding you to run it first.
It is needed because otherwise you would have to press enter every time
you want to send something to the server.


## How to play

Keys:
- WASD or arrow keys: move and rotate
- Ctrl+C, Ctrl+D or Ctrl+Q: quit
- r: change rotating direction

There's only one score; you play together, not against other players.
Try to collaborate and make the best use of everyone's blocks.


## Troubleshooting

- On some systems, the `stty` and `nc` commands must be ran at once using e.g. `&&` as shown above,
    instead of entering them separately.
- If you use a firewall, you may need to tell it to allow listening on
    the port that catris uses.
    For example, for UFW this would be `sudo ufw allow in 12345 comment 'catris'`.
