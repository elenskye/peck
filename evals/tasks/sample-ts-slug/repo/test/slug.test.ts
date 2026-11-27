import { test } from "node:test";
import assert from "node:assert/strict";
import { slug } from "../src/slug.ts";

test("one space", () => {
  assert.equal(slug("Hello World"), "hello-world");
});
