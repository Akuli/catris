# catris

This is a Tetris clone for multiple players that connect to a server with netcat or PuTTY.

![Screenshot](screenshot.png)


## Running the server

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


## Finding the server's IP address

All players must be in the same network with the computer where the server is running.
For example, the computers could be all connected to the same WiFi network,
or connected to the same router with Ethernet cables.
The server computer itself can also be used for playing the game,
just like any other computer in the network.

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

Once you know the server computer's IP address, players can connect to the server:

<details>
<summary>Windows</summary>

[Install PuTTY](https://www.putty.org/).
Once installed, you can open it from the start menu.
Fill in these settings:
- Session:
    - Host Name: Put the IP address of the server here.
    - Port: `12345`
    - Connection type: Raw
- Terminal:
    - Local echo: Force off
    - Local line editing: Force off

Then click the "Open" button to play.

</details>

<details>
<summary>MacOS or Linux</summary>

Open a terminal and run:

```
stty raw; nc PUT_THE_IP_ADDRESS_HERE 12345; stty cooked
```

If you forget `stty raw`, you will get an error message reminding you to run it first.
It is needed because otherwise you would have to press enter every time
you want to send something to the server.

If you forget `stty cooked`, the terminal will be in a weird state when you quit the game,
and you may need to open a new terminal.

</details>


## How to play

Keys:
- WASD or arrow keys: move and rotate
- Ctrl+C, Ctrl+D or Ctrl+Q: quit
- r: change rotating direction
- f: flip the game upside down (only available in ring mode with 1 player)

There's only one score; you play together, not against other players.
Try to collaborate and make the best use of everyone's blocks.


## Troubleshooting

- On some systems, the `stty` and `nc` commands must be ran at once using e.g. `;` as shown above,
    instead of entering them separately.
- Some networks (e.g. the network of the university I go to)
    don't allow computers to connect directly to each other.
    To work around this, just use a different network,
    e.g. your phone's WiFi hotspot.
- If the server computer has a firewall, you may need to tell it to allow listening on port 12345.
    For example, on Windows you can click a button that appears when you run the server for the first time,
    and if you use UFW (quite common on Linux),
    you need to run `sudo ufw allow in 12345 comment 'catris'`.
