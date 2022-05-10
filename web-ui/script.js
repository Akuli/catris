document.addEventListener("DOMContentLoaded", () => {
  const COLORS = {
    // taken from mate-terminal with a color picker program
    '0': {fg: '#FFFFFF', bg: '#000000'},  // reset all colors

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

      this.width = 80;
      this.height = 24;
      this._cursorX = 0;  // 0 <= _cursorX < width
      this._cursorY = 0;  // 0 <= _cursorY < height
      this._cursorIsShowing = true;

      this.fgColor = COLORS['0'].fg;
      this.bgColor = COLORS['0'].bg;
      this.clear();
    }

    clear() {
      this._el.innerHTML = "";
      for (let i = 0; i < this.height; i++) {
        this._el.appendChild(this._makeBlankRow());
      }
    }

    _makeBlankRow() {
      const row = document.createElement("pre");
      row.innerHTML = "<span></span>";
      row.querySelector("span").textContent = " ".repeat(this.width);
      this._applyStyle(row);
      return row;
    }

    _applyStyle(span) {
      span.style.color = this.fgColor;
      span.style.backgroundColor = this.bgColor;
    }

    _fixCursorPos() {
      if (this._cursorX < 0) this._cursorX = 0;
      if (this._cursorY < 0) this._cursorY = 0;
      if (this._cursorX >= this.width) this._cursorX = this.width-1;
      if (this._cursorY >= this.height) this._cursorY = this.height-1;
    }

    moveCursorDown() {
      this._cursorY++;
      this._fixCursorPos();
    }

    // Can be called in a loop, but calls must be in the same order as spans appear in the terminal.
    // May change text of the span given as argument, but doesn't delete it.
    _mergeWithPreviousSpanIfPossible(right) {
      const left = right?.previousElementSibling;
      if (left && right
          && left.style.color === right.style.color
          && left.style.backgroundColor === right.style.backgroundColor)
      {
        right.textContent = left.textContent + right.textContent;
        left.remove();  
      }
    }

    _deleteText(x, y, deleteCount) {
      const spansToRemove = [];
      let spanStart = 0;
      for (const span of this._el.children[y].children) {
        const spanEnd = spanStart + span.textContent.length;

        const overlapStart = Math.max(spanStart, x);
        const overlapEnd = Math.min(spanEnd, x + deleteCount);
        if (overlapStart < overlapEnd) {
          // Delete overlapping part
          const relativeOverlapStart = overlapStart - spanStart;
          const relativeOverlapEnd = overlapEnd - spanStart;
          console.assert(relativeOverlapStart >= 0);
          console.assert(relativeOverlapEnd >= 0);
          const t = span.textContent;
          span.textContent = t.slice(0, relativeOverlapStart) + t.slice(relativeOverlapEnd);
          if (span.textContent === "") {
            // Avoid adding/removing child elements while looping over them
            spansToRemove.push(span);
          }
        }

        spanStart = spanEnd;
      }

      for (const span of spansToRemove) {
        // If another non-empty span got merged into the span on a previous iteration, don't remove
        if (span.textContent === "") {
          const right = span.nextElementSibling;
          span.remove();
          this._mergeWithPreviousSpanIfPossible(right);
        }
      }
    }

    _insertText(x, y, text) {
      if (text === "") {
        return;
      }

      const newSpan = document.createElement("span");
      newSpan.textContent = text;
      this._applyStyle(newSpan);

      const row = this._el.children[y];

      let spanStart = 0;
      for (const span of row.children) {
        if (x === spanStart) {
          // New span goes just before start of an existing span
          row.insertBefore(newSpan, span);
          this._mergeWithPreviousSpanIfPossible(newSpan);
          this._mergeWithPreviousSpanIfPossible(span);
          return;
        }

        const spanEnd = spanStart + span.textContent.length;
        if (spanStart < x && x < spanEnd) {
          // Split span into two, add in middle
          const leftSide = span.cloneNode();
          const rightSide = span.cloneNode();
          leftSide.textContent = span.textContent.slice(0, x - spanStart);
          rightSide.textContent = span.textContent.slice(x - spanStart);

          row.insertBefore(leftSide, span);
          row.insertBefore(newSpan, span);
          row.insertBefore(rightSide, span);
          span.remove();

          this._mergeWithPreviousSpanIfPossible(newSpan);
          this._mergeWithPreviousSpanIfPossible(rightSide);
          return;
        }

        spanStart = spanEnd;
      }

      // New span is not at the start of any span and not inside any span.
      // It must be after all other spans.
      console.assert(x === spanStart);
      row.appendChild(newSpan);
      this._mergeWithPreviousSpanIfPossible(newSpan);
    }

    resize(newWidth, newHeight) {
      for (let y = this.height; y < newHeight; y++) {
        this._el.appendChild(this._makeBlankRow());
      }
      for (let y = this.height-1; y >= newHeight; y--) {
        this._el.children[y].remove();
      }
      this.height = newHeight;

      if (newWidth > this.width) {
        for (let y = 0; y < this.height; y++) {
          this._insertText(this.width, y, " ".repeat(newWidth - this.width));
        }
      }
      if (newWidth < this.width) {
        for (let y = 0; y < this.height; y++) {
          this._deleteText(newWidth, y, this.width - newWidth);
        }
      }
      this.width = newWidth;

      this._el.style.width = newWidth + "ch";
      this._fixCursorPos();
    }

    clearFromCursorToEndOfLine() {
      const n = this.width - this._cursorX;
      this._deleteText(this._cursorX, this._cursorY, n);
      this._insertText(this._cursorX, this._cursorY, " ".repeat(n));
    }

    clearFromCursorToEndOfScreen() {
      this.clearFromCursorToEndOfLine();
      for (let y = this._cursorY + 1; y < this.height; y++) {
        this._deleteText(0, y, this.width);
        this._insertText(0, y, " ".repeat(this.width));
      }
    }

    _addTextRaw(text) {
      console.assert(this._cursorX + text.length <= this.width);
      this._deleteText(this._cursorX, this._cursorY, text.length);
      this._insertText(this._cursorX, this._cursorY, text);
      this._cursorX += text.length;
      this._cursorY += Math.floor(this._cursorX / this.width);
      this._cursorX %= this.width;
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
        this.clear();
      } else if (ansiCode === "\x1b[0J") {
        this.clearFromCursorToEndOfScreen();
      } else if (ansiCode === "\x1b[0K") {
        this.clearFromCursorToEndOfLine();
      } else if (ansiCode === "\x1b[?25h") {
        this._cursorIsShowing = true;
      } else if (ansiCode === "\x1b[?25l") {
        this._cursorIsShowing = false;
      } else if (ansiCode.endsWith("H")) {
        const [line, column] = ansiCode.slice(2, -1).split(';').map(x => +x);
        this._cursorX = column-1;
        this._cursorY = line-1;
        this._fixCursorPos();
      } else if (ansiCode.startsWith("\x1b[1;") && ansiCode.endsWith("m") && COLORS[ansiCode.slice(4, -1)]) {
        const colorInfo = COLORS[ansiCode.slice(4, -1)];
        if (colorInfo.fg) this.fgColor = colorInfo.fg;
        if (colorInfo.bg) this.bgColor = colorInfo.bg;
      } else if (ansiCode.startsWith("\x1b[8;") && ansiCode.endsWith("t")) {
        const [height, width] = ansiCode.slice(4, -1).split(";").map(x => +x);
        this.resize(width, height);
      } else {
        console.warn("Unknown ANSI escape sequence: " + ansiCode);
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
          this.moveCursorDown();
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

  const ws = new WebSocket(`ws://${window.location.hostname}:54321`);

  function sendText(text) {
    const utf8 = new TextEncoder().encode(text);
    if(ws.readyState === WebSocket.OPEN) {
      ws.send(utf8);
    }
  }

  document.addEventListener("paste", event => {
    const pastedText = event.clipboardData.getData("text/plain");
    sendText(pastedText.replace(/\n|\r|\x1b/g, ""));
  });

  document.onkeydown = (event) => {
    if (event.ctrlKey || event.altKey) {
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
    terminal.clear();
    terminal.addTextWithEscapeSequences(
      "\x1b[1;0m"    // reset color
      + "\x1b[2J"    // clear terminal
      + "\x1b[1;1H"  // move cursor to top left corner
      + "Disconnected."
    );
  };

  if (!navigator.userAgent.includes("Windows")) {
    document.querySelector("#netcat-instructions").innerHTML =
      "<p>You can also play this game on a terminal:</p><pre></pre>";
    document.querySelector("#netcat-instructions > pre").textContent =
      `$ stty raw; nc ${window.location.hostname} 12345; stty cooked`;
  }
});
