# catris

This is a Tetris clone for multiple players that connect to a server with netcat.

![Screenshot](screenshot.png)

Install Python and download all files in the `catris` folder.
If you already have Git, you can use it:

```
git clone https://github.com/Akuli/catris
cd catris
```

Or you can [download a zip file by clicking here](https://github.com/Akuli/catris/archive/refs/heads/main.zip),
extract it and `cd` into it.
Either way, you should see a folder named `catris` if you run `dir` or `ls`.

Then run the server (if you're on Windows, use `py` instead of `python3`):

```
python3 -m catris
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
- f: flip the game upside down (only available in ring mode with 1 player)

There's only one score; you play together, not against other players.
Try to collaborate and make the best use of everyone's blocks.


## PuTTY

If you use Windows, you can play the game with telnet on the Windows command prompt,
but it isn't ideal:
- Blue blocks are dark blue and hard to see against the black background.
- Each square looks a bit more wide than tall.
- Drill blocks don't have a gray background like they're supposed to have.

To fix these problems, you can [install PuTTY](https://www.putty.org/).
Use these PuTTY settings to connect to catris:
- Session:
    - Host Name: localhost (or the IP address of a server)
    - Port: 12345
    - Connection type: Raw
- Terminal:
    - Local echo: Force off
    - Local line editing: Force off

Click "Open" after filling in the settings.


## Troubleshooting

- On some systems, the `stty` and `nc` commands must be ran at once using e.g. `&&` as shown above,
    instead of entering them separately.
- If you use a firewall, you may need to tell it to allow listening on
    the port that catris uses.
    For example, for UFW this would be `sudo ufw allow in 12345 comment 'catris'`.
