// A tiny WebSocket server for the integration tests (text frames only, no dependencies), so the
// SDK and bridge tests run with plain `node`. Not for production use.
import { createHash } from "node:crypto";
import { createServer } from "node:http";

/** Starts a server on a free port; `onConnection(socket, request)` gets {send, close, on("message")}. */
export function serve(onConnection) {
  const server = createServer((_, res) => res.writeHead(426).end());
  server.on("upgrade", (request, tcp) => {
    const accept = createHash("sha1").update(request.headers["sec-websocket-key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest("base64");
    const protocol = request.headers["sec-websocket-protocol"]?.split(",")[0].trim();
    tcp.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n${protocol ? `Sec-WebSocket-Protocol: ${protocol}\r\n` : ""}\r\n`);
    const listeners = [];
    const socket = {
      send(text) {
        const body = Buffer.from(text);
        const head = body.length < 126 ? Buffer.from([0x81, body.length])
          : Buffer.from([0x81, 126, body.length >> 8, body.length & 255]);
        tcp.write(Buffer.concat([head, body]));
      },
      close(code = 1000) { tcp.end(Buffer.from([0x88, 2, code >> 8, code & 255])); },
      on(_event, fn) { listeners.push(fn); },
    };
    let buffer = Buffer.alloc(0);
    tcp.on("data", chunk => {
      buffer = Buffer.concat([buffer, chunk]);
      while (buffer.length >= 2) {
        let length = buffer[1] & 127, offset = 2;
        if (length === 126) { length = buffer.readUInt16BE(2); offset = 4; }
        if (buffer.length < offset + 4 + length) return;
        const mask = buffer.subarray(offset, offset + 4);
        const payload = Buffer.from(buffer.subarray(offset + 4, offset + 4 + length).map((b, i) => b ^ mask[i % 4]));
        const opcode = buffer[0] & 15;
        buffer = buffer.subarray(offset + 4 + length);
        if (opcode === 8) { tcp.end(); return; }
        if (opcode === 1) listeners.forEach(fn => fn(payload.toString()));
      }
    });
    tcp.on("error", () => {});
    onConnection(socket, request);
  });
  return new Promise(resolve => server.listen(0, "127.0.0.1", () => resolve({ port: server.address().port, close: () => server.close() })));
}
