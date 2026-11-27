import { test } from "node:test";
import assert from "node:assert/strict";
import { slug } from "../src/slug.ts";

test("every space", () => {
  assert.equal(slug("My Big Title"), "my-big-title");
});

test("a run of spaces counts once", () => {
  assert.equal(slug("a   b"), "a-b");
});

test("trims both ends", () => {
  assert.equal(slug("  Hi there "), "hi-there");
});
