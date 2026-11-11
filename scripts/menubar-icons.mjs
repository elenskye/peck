#!/usr/bin/env node
// 菜单栏模板图：把 src/vendor/peck/marks/menubar/*.svg（18pt 像素格，全是 <rect>）出成 @2x 的 PNG，
// 放进 src-tauri/icons/menubar/。黑 + 透明，由系统上色。视觉换了版本再跑一次：node scripts/menubar-icons.mjs
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { deflateSync } from "node:zlib";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = path.join(root, "src/vendor/peck/marks/menubar");
const target = path.join(root, "src-tauri/icons/menubar");
const SCALE = 2;
const NAMES = ["idle", "run-1", "run-2", "run-3", "wait"];

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const byte of buf) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const head = Buffer.alloc(8);
  head.writeUInt32BE(data.length, 0);
  head.write(type, 4, "ascii");
  const tail = Buffer.alloc(4);
  tail.writeUInt32BE(crc32(Buffer.concat([head.subarray(4), data])), 0);
  return Buffer.concat([head, data, tail]);
}
function png(width, height, rgba) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 6, 0, 0, 0], 8); // 8 位、RGBA
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

mkdirSync(target, { recursive: true });
for (const name of NAMES) {
  const svg = readFileSync(path.join(source, `${name}.svg`), "utf8");
  const [, w, h] = svg.match(/viewBox="0 0 (\d+) (\d+)"/).map(Number);
  const width = w * SCALE;
  const height = h * SCALE;
  const rgba = Buffer.alloc(width * height * 4);
  for (const [, attrs] of svg.matchAll(/<rect ([^>]*)>/g)) {
    const num = (key, fallback) => {
      const found = attrs.match(new RegExp(`${key}="([\\d.]+)"`));
      return found ? Number(found[1]) : fallback;
    };
    const alpha = Math.round(num("opacity", 1) * 255);
    const x0 = num("x", 0) * SCALE;
    const y0 = num("y", 0) * SCALE;
    for (let y = y0; y < y0 + num("height", 0) * SCALE; y++) {
      for (let x = x0; x < x0 + num("width", 0) * SCALE; x++) {
        rgba[(y * width + x) * 4 + 3] = alpha; // RGB 留 0（黑），只写透明度
      }
    }
  }
  writeFileSync(path.join(target, `${name}.png`), png(width, height, rgba));
  console.log(`${name}.png ${width}×${height}`);
}
