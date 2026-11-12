// Generates a fixed extension identity: prints the manifest "key" and the resulting extension ID.
// The private key is discarded on purpose; an unpacked extension only needs the public key.
const crypto = require("node:crypto");

const { publicKey } = crypto.generateKeyPairSync("rsa", { modulusLength: 2048 });
const der = publicKey.export({ type: "spki", format: "der" });
const id = [...crypto.createHash("sha256").update(der).digest().subarray(0, 16)]
  .map(byte => byte.toString(16).padStart(2, "0"))
  .join("")
  .replace(/[0-9a-f]/g, nibble => String.fromCharCode(97 + parseInt(nibble, 16)));

console.log(JSON.stringify({ key: der.toString("base64"), id }, null, 2));
