# catris

This is a Tetris clone for multiple players that connect to a server with netcat.

![Screenshot](screenshot.png)

First, run the server:

```
$ git clone https://github.com/Akuli/catris
$ cd catris
$ python3 catris.py
```

To connect to it, open a new terminal and run:

```
$ stty raw
$ nc localhost 12345
```

The port is literally `12345`.
If the server is running on a different computer,
replace `localhost` with the server's IP or hostname.

If you forget `stty raw`, you will get an error message reminding you to run it first.
It is needed because otherwise you would have to press enter every time
you want to send something to the server.

You may need to allow connections to the server through your firewall.
For example:

```
$ sudo ufw allow in 12345 comment 'catris'
```
