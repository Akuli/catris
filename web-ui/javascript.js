document.addEventListener("DOMContentLoaded", () => {
  const COLORS = {
    // taken from mate-terminal with a color picker program
    '30': {fg: '#555753'},
    '31': {fg: '#EF2929'},
    '32': {fg: '#8AE234'},
    '33': {fg: '#FCE94F'},
    '34': {fg: '#729FCF'},
    '35': {fg: '#AD7FA8'},
    '36': {fg: '#34E2E2'},

    '40': {bg: '#2E3436'},
    '41': {bg: '#CC0000'},
    '42': {bg: '#4E9A06'},
    '43': {bg: '#C4A000'},
    '44': {bg: '#3465A4'},
    '45': {bg: '#75507B'},
    '46': {bg: '#06989A'},
    '47': {bg: '#D3D7CF'},

    // 3x and 9x are the same colors
    '90': {fg: '#555753'},
    '91': {fg: '#EF2929'},
    '92': {fg: '#8AE234'},
    '93': {fg: '#FCE94F'},
    '94': {fg: '#729FCF'},
    '95': {fg: '#AD7FA8'},
    '96': {fg: '#34E2E2'},

    // In general, 10x colors are brighter than 4x colors.
    //
    // Some (not all) of the 10x background colors are same as 3x and 9x foreground colors.
    // I don't know why this is, but I do what mate-terminal does :)
    '100': {bg: '#555753'},
    '101': {bg: '#EF2929'},
    '102': {bg: '#81D431'},
    '103': {bg: '#FCE94F'},
    '104': {bg: '#729FCF'},
    '105': {bg: '#AD7FA8'},
    '106': {bg: '#34E2E2'},
    '107': {bg: '#EEEEEC'},
  };

  function splitToLines(text, firstLineLen, consecutiveLinesLen) {
    const result = [text.slice(0, firstLineLen)];
    let start = firstLineLen;
    while (start < text.length) {
      const end = start + consecutiveLinesLen;
      result.push(text.slice(start, end));
      start = end;
    }
    return result;
  }

  const terminal = new class {
    constructor() {
      this._el = document.getElementById("terminal");
      this._cache = [];

      this.width = 0;
      this.height = 0;
      this._cursorX = 0;  // after initial resize: 0 <= _cursorX < width
      this._cursorY = 0;  // after initial resize: 0 <= _cursorY < height
      this._cursorIsShowing = true;
      this._resetColors();
      this._resize(80, 24);
    }

    _resetColors() {
      this.fgColor = '#FFFFFF';
      this.bgColor = '#000000';
    }

    _fixCursorPos() {
      if (this._cursorX < 0) this._cursorX = 0;
      if (this._cursorY < 0) this._cursorY = 0;
      if (this._cursorX >= this.width) this._cursorX = this.width-1;
      if (this._cursorY >= this.height) this._cursorY = this.height-1;
    }

    _makeBlankCell() {
      const cell = document.createElement("span");
      cell.textContent = " ";
      cell.style.color = this.fgColor;
      cell.style.backgroundColor = this.bgColor;
      return { cell, text: " ", fg: this.fgColor, bg: this.bgColor };
    }

    _writeCell(x, y, text) {
      console.assert(text.length === 1);
      const cacheItem = this._cache[y][x];

      // Editing and querying DOM elements is slow, avoid that
      if (cacheItem.fg !== this.fgColor) {
        cacheItem.fg = this.fgColor;
        cacheItem.cell.style.color = this.fgColor;
      }
      if (cacheItem.bg !== this.bgColor) {
        cacheItem.bg = this.bgColor;
        cacheItem.cell.style.backgroundColor = this.bgColor;
      }
      if (cacheItem.text !== text) {
        cacheItem.text = text;
        cacheItem.cell.textContent = text;
      }
    }

    _resize(newWidth, newHeight) {
      for (let y = this.height; y < newHeight; y++) {
        const elementRow = document.createElement("pre");
        const cacheRow = Array(this.width).fill().map(() => this._makeBlankCell());
        for (const cacheItem of cacheRow) {
          elementRow.appendChild(cacheItem.cell);
        }
        this._el.appendChild(elementRow);
        this._cache.push(cacheRow);
      }

      for (let y = this.height-1; y >= newHeight; y--) {
        this._el.children[y].remove();
        this._cache.pop();
      }
      this.height = newHeight;

      for (let y = 0; y < this.height; y++) {
        const cacheRow = this._cache[y];
        const elementRow = this._el.children[y];
        for (const cacheItem of cacheRow.splice(newWidth)) {
          cacheItem.cell.remove();
        }
        for (let x = this.width; x < newWidth; x++) {
          const cacheItem = this._makeBlankCell();
          cacheRow.push(cacheItem);
          elementRow.appendChild(cacheItem.cell);
        }
      }
      this.width = newWidth;

      this._el.style.width = newWidth + "ch";
      this._fixCursorPos();
    }

    _clear() {
      for (let y = 0; y < this.height; y++) {
        for (let x = 0; x < this.width; x++) {
          this._writeCell(x, y, " ");
        }
      }
    }

    _clearFromCursorToEndOfLine() {
      for (let x = this._cursorX; x < this.width; x++) {
        this._writeCell(x, this._cursorY, " ");
      }
    }

    clearFromCursorToEndOfScreen() {
      this._clearFromCursorToEndOfLine();
      for (let y = this._cursorY + 1; y < this.height; y++) {
        for (let x = 0; x < this.width; x++) {
          this._writeCell(x, y, " ");
        }
      }
    }

    _addTextRaw(text) {
      console.assert(this._cursorX + text.length <= this.width);
      for (const character of text) {
        this._writeCell(this._cursorX, this._cursorY, character);
        this._cursorX++;
      }
      if (this._cursorX === this.width) {
        this._cursorX = 0;
        this._cursorY++;
      }
      this._fixCursorPos();
    }

    _updateCursor() {
      if (this._cursorIsShowing) {
        document.getElementById("cursor-overlay").textContent =
          "\n".repeat(this._cursorY) + " ".repeat(this._cursorX) + "█";
      } else {
        document.getElementById("cursor-overlay").textContent = "";
      }
    }

    _handleAnsiCode(ansiCode) {
      if (ansiCode === "\x1b[2J") {
        this._clear();
      } else if (ansiCode === "\x1b[0J") {
        this.clearFromCursorToEndOfScreen();
      } else if (ansiCode === "\x1b[0K") {
        this._clearFromCursorToEndOfLine();
      } else if (ansiCode === "\x1b[?25h") {
        this._cursorIsShowing = true;
      } else if (ansiCode === "\x1b[?25l") {
        this._cursorIsShowing = false;
      } else if (ansiCode.endsWith("H")) {
        const [line, column] = ansiCode.slice(2, -1).split(';').map(x => +x);
        this._cursorX = column-1;
        this._cursorY = line-1;
        this._fixCursorPos();
      } else if (ansiCode.endsWith("G")) {
        this._cursorX = (+ansiCode.slice(2, -1)) - 1;
        this._fixCursorPos();
      } else if (ansiCode.startsWith("\x1b[1;") && ansiCode.endsWith("m") && COLORS[ansiCode.slice(4, -1)]) {
        const colorInfo = COLORS[ansiCode.slice(4, -1)];
        if (colorInfo.fg) this.fgColor = colorInfo.fg;
        if (colorInfo.bg) this.bgColor = colorInfo.bg;
      } else if (ansiCode == "\x1b[0m") {
        this._resetColors();
      } else if (ansiCode.startsWith("\x1b[8;") && ansiCode.endsWith("t")) {
        const [height, width] = ansiCode.slice(4, -1).split(";").map(x => +x);
        this._resize(width, height);
      } else {
        console.warn(`Unknown ANSI escape sequence: '${ansiCode}'`);
      }
    }

    addTextWithEscapeSequences(text) {
      // UTF-8 characters, ANSI escape sequences etc are never received in two parts.
      // This is because of how websockets work, and isn't true for raw TCP connections.
      const chunks = text.match(/\x1b\[[^A-Za-z]+[A-Za-z]|\r|\n|[^\x1b\r\n]+/g);
      for (const chunk of chunks) {
        if (chunk === "\r") {
          this._cursorX = 0;
        } else if (chunk === "\n") {
          this._cursorY++;
          this._fixCursorPos();
        } else if (chunk.startsWith("\x1b[")) {
          this._handleAnsiCode(chunk);
        } else {
          const lines = splitToLines(chunk, this.width - this._cursorX, this.width);
          for (const line of lines) {
            this._addTextRaw(line);
          }
        }
      }

      this._updateCursor();
    }
  };

  let wsUrl;
  if (window.location.hostname === 'catris.net') {
    // Production websocket uses same port as static files.
    // nginx redirects websocket connections to backend
    const protocol = window.location.protocol==='https:' ? 'wss' : 'ws';
    wsUrl = `${protocol}://${window.location.host}/websocket`;
  } else {
    // Backend listens to websocket on port 54321.
    // Can be localhost, or a different computer (see local-playing.md)
    wsUrl = `ws://${window.location.hostname}:54321/websocket`;
  }
  const ws = new WebSocket(wsUrl);

  function sendText(text) {
    const utf8 = new TextEncoder().encode(text);
    if(ws.readyState === WebSocket.OPEN && utf8.length > 0) {
      ws.send(utf8);
    }
  }

  document.addEventListener("paste", event => {
    sendText(event.clipboardData.getData("text/plain"));
  });

  document.onkeydown = (event) => {
    // metaKey is the command key on MacOS
    if (event.ctrlKey || event.altKey || event.metaKey) {
      return;
    }

    if (event.key.length === 1) {
      sendText(event.key);
    } else if (event.key === "ArrowUp") {
      sendText("\x1b[A");
    } else if (event.key === "ArrowDown") {
      sendText("\x1b[B");
    } else if (event.key === "ArrowRight") {
      sendText("\x1b[C");
    } else if (event.key === "ArrowLeft") {
      sendText("\x1b[D");
    } else if (event.key === "Backspace") {
      sendText("\x7f");
    } else if (event.key === "Enter") {
      sendText("\r");
    } else {
      console.log("Unrecognized key:", event.key);
      return;
    }

    event.preventDefault();
  };

  // For some reason, the .text() method on Blob is asynchronous.
  // We need to make sure they run in the correct order, and not at the same time.

  let receivedTextPromises = [];
  let handleBlobsRunning = false;

  async function handleBlobs() {
    while (receivedTextPromises.length !== 0) {
      const promises = receivedTextPromises;
      receivedTextPromises = [];
      const text = (await Promise.all(promises)).join("");
      if (ws.readyState === WebSocket.OPEN) {
        terminal.addTextWithEscapeSequences(text);
      }
    }
    handleBlobsRunning = false;
  }

  ws.onmessage = (msg) => {
    receivedTextPromises.push(msg.data.text());
    if (!handleBlobsRunning) {
      handleBlobsRunning = true;
      handleBlobs();
    }
  };

  // no need for onerror(), because onclose() will run on errors too
  ws.onclose = () => {
    terminal.addTextWithEscapeSequences(
      "\x1b[1;0m"    // reset color
      + "\x1b[2J"    // clear terminal
      + "\x1b[1;1H"  // move cursor to top left corner
      + "Disconnected."
    );
  };

  if (!navigator.userAgent.includes("Windows")) {
    const nc = document.getElementById("netcat-instructions");
    nc.innerHTML = "<p>You can also play this game on a terminal:</p><pre></pre>";
    nc.querySelector("pre").textContent = `$ stty raw; nc ${window.location.hostname} 12345; stty cooked`;
  }
});
