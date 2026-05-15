import * as cheerio from "cheerio";
import { absolutize, fetchText } from "./http.js";
import type { DocumentDetail, SearchResponse, SearchResult } from "./types.js";
import { SourceError } from "./types.js";

export interface CiaSearchParams {
  query: string;
  max_results: number;
  cursor?: string;
  base_url: string;
}

function parseCursor(cursor?: string): number {
  if (!cursor) return 0;
  const page = Number.parseInt(Buffer.from(cursor, "base64url").toString("utf8"), 10);
  return Number.isFinite(page) && page >= 0 ? page : 0;
}

function makeCursor(page: number): string {
  return Buffer.from(String(page), "utf8").toString("base64url");
}

export function parseCiaSearch(html: string, base_url: string, query: string, page: number): SearchResponse {
  const $ = cheerio.load(html);
  const results: SearchResult[] = [];

  const candidates = $(".search-result, .views-row, article, li.search-result").toArray();
  for (const el of candidates) {
    const item = $(el);
    const link = item.find("a[href*='/readingroom/document/']").first();
    if (!link.length) continue;
    const href = link.attr("href") ?? "";
    const url = absolutize(href, base_url);
    const title = link.text().replace(/\s+/g, " ").trim() || item.find("h3,h2").first().text().trim();
    const id = url.split("/document/")[1]?.split(/[?#]/)[0] ?? url;
    const text = item.text().replace(/\s+/g, " ").trim();
    const pdf = item.find("a[href$='.pdf'], a[href*='.pdf']").first().attr("href");
    results.push({
      source: "cia",
      id,
      title: title || id,
      url,
      document_url: url,
      pdf_url: pdf ? absolutize(pdf, base_url) : undefined,
      description: text.slice(0, 500),
    });
  }

  const hasNext =
    $("a[rel='next'], .pager-next a, a:contains('next'), a:contains('Next')").length > 0 ||
    results.length > 0;
  return {
    query,
    source: "cia_reading_room",
    results,
    next_cursor: hasNext ? makeCursor(page + 1) : undefined,
    warnings:
      results.length === 0
        ? [
            "CIA Reading Room HTML shape may have changed or blocked scraping. Try the same query manually on the source site.",
          ]
        : undefined,
  };
}

export async function searchCiaReadingRoom(params: CiaSearchParams): Promise<SearchResponse> {
  const page = parseCursor(params.cursor);
  const path = `/readingroom/search/site/${encodeURIComponent(params.query)}`;
  const url = new URL(path, params.base_url);
  if (page > 0) url.searchParams.set("page", String(page));
  const html = await fetchText(url.toString(), { source: "CIA Reading Room" });
  const parsed = parseCiaSearch(html, params.base_url, params.query, page);
  parsed.results = parsed.results.slice(0, params.max_results);
  return parsed;
}

function textClean(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

export function parseCiaDocument(html: string, base_url: string, fallback_url: string): DocumentDetail {
  const $ = cheerio.load(html);
  const canonical = $("link[rel='canonical']").attr("href");
  const url = canonical ? absolutize(canonical, base_url) : fallback_url;
  const id = url.split("/document/")[1]?.split(/[?#]/)[0] ?? fallback_url;
  const title =
    textClean($("h1").first().text()) ||
    textClean($("title").first().text()) ||
    id;
  const metadata: Record<string, unknown> = {};

  $(".field, .document-meta, .metadata, dl").each((_idx, el) => {
    const node = $(el);
    const label =
      textClean(node.find(".field-label, dt, label").first().text()).replace(/:$/, "") ||
      textClean(node.find("strong").first().text()).replace(/:$/, "");
    const value =
      textClean(node.find(".field-item, dd").first().text()) ||
      textClean(node.text()).replace(label, "").trim();
    if (label && value && label.length < 80) metadata[label] = value;
  });

  const attachments = $("a[href$='.pdf'], a[href*='.pdf'], a[href*='/docs/']")
    .toArray()
    .map((el) => {
      const link = $(el);
      const href = link.attr("href") ?? "";
      return {
        label: textClean(link.text()) || "document",
        url: absolutize(href, base_url),
        type: href.toLowerCase().includes(".pdf") ? "pdf" : "document",
      };
    })
    .filter((item, index, array) => array.findIndex((other) => other.url === item.url) === index);

  const bodyText = textClean($("main, article, .region-content, body").first().text());
  return {
    source: "cia",
    id,
    title,
    url,
    document_url: url,
    pdf_url: attachments.find((item) => item.type === "pdf")?.url,
    metadata,
    attachments,
    text_preview: bodyText.slice(0, 2000),
    citation_note: "CIA FOIA Electronic Reading Room. Verify OCR and redactions against original scan/PDF.",
  };
}

export async function getCiaDocument(id_or_url: string, base_url: string): Promise<DocumentDetail> {
  const url = id_or_url.startsWith("http")
    ? id_or_url
    : new URL(`/readingroom/document/${encodeURIComponent(id_or_url)}`, base_url).toString();
  if (!url.includes("/readingroom/document/")) {
    throw new SourceError(
      "CIA document lookup expects a Reading Room document id or /readingroom/document/ URL.",
      "cia",
      undefined,
      "Pass ids like cia-rdp68r00530a000200110020-2.",
    );
  }
  const html = await fetchText(url, { source: "CIA Reading Room" });
  return parseCiaDocument(html, base_url, url);
}
