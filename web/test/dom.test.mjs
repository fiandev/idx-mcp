// The terminal renders a few lines as markup (`el(..., { html })`). Those lines
// interpolate provider strings — issuer names, sector names, symbols — so the
// `h` tagged template must escape every value it inserts. A quote character or
// a `<script>` in a company name must stay text, never become markup.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import { esc, h } from "../public/app.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

test("esc neutralises the characters that can open markup", () => {
  assert.equal(esc(`<img src=x onerror="alert(1)">`), "&lt;img src=x onerror=&quot;alert(1)&quot;&gt;");
  assert.equal(esc("A & B"), "A &amp; B");
  assert.equal(esc("it's"), "it&#39;s");
  assert.equal(esc(null), "");
  assert.equal(esc(undefined), "");
  assert.equal(esc(42), "42");
});

test("every html: in the terminal goes through h`…`", () => {
  const src = readFileSync(join(root, "web", "public", "terminal.js"), "utf8");
  const sites = src.split("\n").filter((l) => l.includes("html:"));
  assert.ok(sites.length > 0, "the terminal still builds some markup");
  for (const line of sites) {
    assert.ok(line.includes("html: h`"), `raw markup bypasses the escaper: ${line.trim()}`);
  }
  // A local named `h` would shadow the tag and turn `h`…` into a call.
  const shadow = [...src.matchAll(/\b(?:const|let|var)\b[^;\n]*?\bh\s*=/g)];
  assert.deepEqual(shadow.map((m) => m[0]), []);
});

test("h keeps the template's own markup and escapes only the values", () => {
  const name = `PT <b>Bank</b> & "Tbk"`;
  assert.equal(
    h`<b>${name}</b>`,
    "<b>PT &lt;b&gt;Bank&lt;/b&gt; &amp; &quot;Tbk&quot;</b>",
  );
});