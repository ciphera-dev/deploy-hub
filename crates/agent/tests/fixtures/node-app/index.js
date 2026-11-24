// Application de test du build Railpack : repond « ok » sur le port 8080.
const http = require("node:http");

http
  .createServer((_req, res) => {
    res.end("ok\n");
  })
  .listen(Number(process.env.PORT) || 8080);
