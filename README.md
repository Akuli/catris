# catris

This is a Tetris clone for multiple players that connect to a server with netcat or a web interface.

![Screenshot](screenshot.png)

If you aren't using Windows,
you can play on my server by running this command on a terminal:

```
$ stty raw; nc 172.104.132.97 12345; stty cooked
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
You can also type `akuli.github.io/catris` to your browser's address bar.

My server is in Europe, so the game may be very laggy if you're not in Europe.
Please create an issue if this is a problem for you.


## Development

You need to install rust. Just google for some instructions.
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

If you changed rust code, build the executable and copy it to the server:

```
$ cargo build --release
$ scp target/release/catris catris:/home/catris/catris
```

If you modified the web UI, copy the contents of the `web-ui` directory to the server:

```
$ scp -r web-ui/* catris:/var/www/html/
```
