# Playing catris locally

If you don't want to connect to my catris server,
it is possible to play catris on a Local Area Network (LAN),
such as a WiFi network or
with several computers connected to the same router with Ethernet cables.
This file explains how to do that.


## Running the server

If you're on Windows, use `py` instead of `python3` and `env\Scripts\activate` instead of `source env/bin/activate` below.

Install Python and download this repository.
If you already have Git, you can use it:

```
git clone https://github.com/Akuli/catris
cd catris
```

Or you can [download a zip file by clicking here](https://github.com/Akuli/catris/archive/refs/heads/main.zip),
extract it and `cd` into it.
Either way, if you run `dir` or `ls`,
you should see files named `local-playing.md` and `README.md` among other things.
If you see a file named `__init__.py`, you have `cd`'d too deep
and you need to go back up with `cd ..`.

Next, create a virtualenv and install the dependencies.
This is needed for the web interface,
but you don't need to do this if all players will connect with netcat.

```
python3 -m venv env
source env/bin/activate
pip install -r requirements.txt
```

We are now ready to run the server:

```
python3 -m catris
```

If you want to use the web UI, you also need to start a web server in a second terminal.
The server must be started from the `web-ui` directory.

```
cd web-ui
python3 -m http.server
```


## Finding the server's IP address

Next you need to know the server's IP address within the local network.
When connecting to the server, players need to specify this IP address.
It is **not** same as the network's public IP,
which is what you find by googling "what is my ip" or similar.

If the server is running Windows, you can run `ipconfig` on a command prompt.
On Linux, you can use `ip a`, and I think `ifconfig` works on MacOS
(if you have a mac, please tell me if it works and whether other commands mentioned here work too).
Either way, you will get messy output with the IP address buried somewhere in the middle of it.
For example, on my Linux computer, `ip a` outputs:

```
1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN group default qlen 1000
    link/loopback 00:00:00:00:00:00 brd 00:00:00:00:00:00
    inet 127.0.0.1/8 scope host lo
       valid_lft forever preferred_lft forever
    inet6 ::1/128 scope host 
       valid_lft forever preferred_lft forever
2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc pfifo_fast state UP group default qlen 1000
    link/ether 10:60:4b:82:57:01 brd ff:ff:ff:ff:ff:ff
    inet 192.168.1.3/24 brd 192.168.1.255 scope global dynamic noprefixroute eth0
       valid_lft 67299sec preferred_lft 67299sec
    inet6 fe80::ab58:f098:59e0:621f/64 scope link noprefixroute 
       valid_lft forever preferred_lft forever
```

Here `192.168.1.3` is my computer's IP address.
It is **NOT** `127.0.0.1`, as that IP address means "this computer",
and it won't connect other computers to the server.


## Connecting to the server

Web UI: Go to `http://SERVER_IP:8000/` in a browser,
where `SERVER_IP` is the IP address of the server computer in the local network.

Terminal UI (doesn't work on Windows): `stty raw; nc SERVER_IP 12345; stty cooked`

The server computer itself can also be used for playing the game,
just like any other computer in the network.


## Troubleshooting

- Some networks (e.g. the network of the university I go to)
    don't allow computers to connect directly to each other.
    To work around this, just use a different network,
    e.g. your phone's WiFi hotspot.
- If the server computer has a firewall, you may need to tell it to allow listening on port 12345.
    For example, on Windows you can click a button that appears when you run the server for the first time,
    and if you use UFW (quite common on Linux),
    you need to run:
    ```
    sudo ufw allow in 8000 comment 'catris http'
    sudo ufw allow in 54321 comment 'catris websocket'
    sudo ufw allow in 12345 comment 'catris raw tcp'
    ```
    If you don't want to use the web UI, you only need to allow port 12345.
