import assert from "node:assert/strict";
import { test } from "node:test";
import { parseCiaDocument, parseCiaSearch } from "../dist/cia.js";

test("parseCiaSearch extracts document results", () => {
  const html = `
    <div class="search-result">
      <h3><a href="/readingroom/document/cia-rdp-test">Weather Modification</a></h3>
      <p>Released memo text.</p>
      <a href="/readingroom/docs/CIA-RDP-TEST.pdf">PDF</a>
    </div>`;
  const parsed = parseCiaSearch(html, "https://www.cia.gov", "weather modification", 0);
  assert.equal(parsed.results.length, 1);
  assert.equal(parsed.results[0].id, "cia-rdp-test");
  assert.equal(parsed.results[0].pdf_url, "https://www.cia.gov/readingroom/docs/CIA-RDP-TEST.pdf");
});

test("parseCiaDocument extracts title and attachments", () => {
  const html = `
    <html>
      <head><link rel="canonical" href="/readingroom/document/cia-rdp-test"></head>
      <body>
        <main>
          <h1>Climate Control</h1>
          <div class="field"><span class="field-label">Document Type:</span><span class="field-item">CREST</span></div>
          <a href="/readingroom/docs/CIA-RDP-TEST.pdf">Download</a>
          <p>OCR preview text.</p>
        </main>
      </body>
    </html>`;
  const doc = parseCiaDocument(html, "https://www.cia.gov", "https://www.cia.gov/readingroom/document/cia-rdp-test");
  assert.equal(doc.id, "cia-rdp-test");
  assert.equal(doc.title, "Climate Control");
  assert.equal(doc.pdf_url, "https://www.cia.gov/readingroom/docs/CIA-RDP-TEST.pdf");
  assert.equal(doc.metadata["Document Type"], "CREST");
});
