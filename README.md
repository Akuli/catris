# catris

This is a Tetris clone for multiple players that connect to a server with netcat or a web interface.
You can play it here: https://akuli.github.io/catris/

![Screenshot](screenshot.png)

My server is in Europe, so the game may be very laggy if you're not in Europe.
Please create an issue if this is a problem for you.


## High-level overview of the code

When the rust program starts, `main()` starts listening on two TCP ports,
54321 for websocket connections and 12345 for plain TCP connections (e.g. netcat).
The `web-ui/` folder contains static files served by nginx,
and the javascript code in `web-ui/` connects a websocket to port 54321.

After a client connects, it mostly doesn't matter whether they use
a websocket connection or a plain TCP connection,
as `connection.rs` abstracts the differences away.
Both connections use [ANSI escape codes](https://en.wikipedia.org/wiki/ANSI_escape_code) defined in `ansi.rs`.
This means that the javascript code in `web-ui/` must interpret ANSI codes,
but it simplifies the rust code a lot.

Next a `Client` object is created.
It is possible to receive and (indirectly) send through a `Client` object.
Specifically, `connection.rs` provides a method to receive a single key press,
and the `Client` object re-exposes it.
For sending, the `Client` has a `RenderData`.
Instead of sending bytes with `connection.rs`,
you usually set the `RenderData`'s `RenderBuffer` to what you want the user to see,
and then fire a `Notify` which causes a task in `main.rs` to actually send screen updates.
Only the changes are sent, the entire screen isn't redrawn every time.

Next:
- We ask the client's name.
- We ask whether the client wants to create a lobby or join an existing lobby.
- If the client wants to join an existing lobby, we ask its ID and join it.
- In the lobby, the client chooses a game.
- The client plays the game, using `ingame_ui.rs` to keep the `RenderBuffer` up to date.

Each item in the above list is a function in `views.rs`.
These functions take the `Client` as an argument, and send and receive through it.

Clients own their lobbies: a lobby is dropped automatically when all of its clients disconnect.
The lobby also knows about what clients it has, but it only contains `ClientInfo` objects,
not actual `Client` objects.
Unlike `Client` objects, the `ClientInfo` objects can't be used to send or receive;
they are purely information for game logic and other clients.

A lobby owns `GameWrapper`s, which take care of the timing and async aspects of a game:
the underlying `Game` objects from `game_logic.rs` are pure logic.
For example, there are several async functions in `game_wrapper`
that call a method of `Game` repeatedly
to e.g. move the blocks down or increment counters on bombs.

The `Game` object also has `Player`s, and each `Player` has a `MovingBlock`.
Moving blocks and landed squares are both `SquareContent` objects.
These are all purely logic, not e.g. async or IO,
so the game logic is split into 3 files: `game_logic.rs`, `player.rs` and `squares.rs`.

When a game ends, the `GameWrapper` records the game results by calling a function in `high_scores.rs`,
and sets the `GameWrapper`'s status so that `views.rs` notices it and displays the high scores.
When the client is done with looking at high scores, they go back to choosing a game.


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
