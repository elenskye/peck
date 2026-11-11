#!/usr/bin/env node
// 菜单栏模板图：把 src/vendor/peck/marks/menubar/*.svg（18 格的框，全是 <rect>）出成 @2x 的 PNG，
// 放进 src-tauri/icons/menubar/。黑 + 透明，由系统上色。视觉换了版本再跑一次：node scripts/menubar-icons.mjs
//
// Tauri 的托盘把图片高度一律压成 18pt。原样出图时鸟只占 18 格里的 10×9 格（一格 1pt），在菜单栏里偏小；
// 所以先裁掉四周的空（几个状态共用同一个范围，鸟不跳），一格出 3 像素：菜单栏里一格 1.5pt，
// 鸟 15×13.5pt，和系统图标差不多大，Retina 屏上还是整像素。
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { deflateSync } from "node:zlib";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = path.join(root, "src/vendor/peck/marks/menubar");
const target = path.join(root, "src-tauri/icons/menubar");
const SCALE = 3; // 一格几个像素（@2x 下一格 1.5pt）
const HEIGHT = 36; // 托盘压成 18pt 高，36 像素正好 @2x
const PAD = 1; // 左右各留一格
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

function readRects(name) {
  const svg = readFileSync(path.join(source, `${name}.svg`), "utf8");
  return [...svg.matchAll(/<rect ([^>]*)>/g)].map(([, attrs]) => {
    const num = (key, fallback) => {
      const found = attrs.match(new RegExp(`${key}="([\\d.]+)"`));
      return found ? Number(found[1]) : fallback;
    };
    return {
      x: num("x", 0),
      y: num("y", 0),
      w: num("width", 0),
      h: num("height", 0),
      alpha: Math.round(num("opacity", 1) * 255),
    };
  });
}

const frames = NAMES.map((name) => ({ name, rects: readRects(name) }));
const all = frames.flatMap((frame) => frame.rects);
// 共用的裁法：左边和上下按所有状态里最外的格子算，右边每张图各自到自己的最后一格。
const left = Math.min(...all.map((r) => r.x)) - PAD;
const top = Math.min(...all.map((r) => r.y));
const bottom = Math.max(...all.map((r) => r.y + r.h));
const contentHeight = (bottom - top) * SCALE;
if (contentHeight > HEIGHT) throw new Error(`内容 ${contentHeight} 像素高，放不进 ${HEIGHT}`);
const offsetY = Math.ceil((HEIGHT - contentHeight) / 2); // 多出来的一像素给上面，和原图一样略偏下

mkdirSync(target, { recursive: true });
for (const { name, rects } of frames) {
  const right = Math.max(...rects.map((r) => r.x + r.w)) + PAD;
  const width = (right - left) * SCALE;
  const height = HEIGHT;
  const rgba = Buffer.alloc(width * height * 4);
  for (const r of rects) {
    const x0 = (r.x - left) * SCALE;
    const y0 = (r.y - top) * SCALE + offsetY;
    for (let y = y0; y < y0 + r.h * SCALE; y++) {
      for (let x = x0; x < x0 + r.w * SCALE; x++) {
        rgba[(y * width + x) * 4 + 3] = r.alpha; // RGB 留 0（黑），只写透明度
      }
    }
  }
  writeFileSync(path.join(target, `${name}.png`), png(width, height, rgba));
  console.log(`${name}.png ${width}×${height}（菜单栏里 ${width / 2}×${height / 2}pt）`);
}
