# catris

This is a Tetris clone for multiple players that connect to a server with netcat or a web interface.
You can play it here: https://akuli.github.io/catris/

![Screenshot](screenshot.png)

My server is in Europe, so the game may be very laggy if you're not in Europe.
Please create an issue if this is a problem for you.


## Development

You need to install rust (the compiler, not the game). Just google for some instructions.
I'm on a Debian-based linux distro, so I first tried `sudo apt install cargo`,
but it was too old and I had to use `rustup` instead.
You might have better luck with your distro's package manager
if you're reading this a few years after I wrote this
or if you're using a different distro.

Once you have rust, you can start the server as you would expect:

```
$ git clone https://github.com/Akuli/catris
$ cd catris
$ cargo r
```

When the server is running, you can connect to it with netcat:

```
$ stty raw; nc localhost 12345; stty cooked
```

If you want to develop the web UI, you need to run a web server in a separate terminal.
You need to have Python installed for this (or you could use some other web server instead).
If you're on Windows, use `py` instead of `python3`.

```
$ cd catris/web-ui
$ python3 -m http.server
```

You can then open `http://localhost:8000/` in your web browser.

I use `cargo fmt` to format my code. GitHub Actions ensures that it was used.


## Deploying

These instructions are mostly for me.
If you want to run catris in a local network, see [local-playing.md](local-playing.md).
If you run a bigger catris server, please let me know by creating an issue :)

Tag a release (use the correct version number, of course):

```
$ your_favorite_editor Cargo.toml
$ git add Cargo.toml
$ git commit -m "Bump version to 4.2.0"
$ git tag v4.2.0
$ git push --tags origin main
```

Look at `journalctl -fu catris` to make sure nobody is currently playing.
If in the future there is always someone playing,
use `/home/catris/catris_motd.txt` to clearly announce the downtime beforehand.

If you changed rust code, build the executable, copy it to the server, and restart the systemd service:

```
$ cargo build --release
$ scp target/release/catris catris:/home/catris/catris
$ ssh catris
$ sudo systemctl restart catris
```

If you modified the web UI, copy the contents of the `web-ui` directory to the server:

```
$ scp -r web-ui/* catris:/var/www/html/
```
