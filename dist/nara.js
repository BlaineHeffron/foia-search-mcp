import { fetchText } from "./http.js";
function decodeCursor(cursor) {
    if (!cursor)
        return 0;
    const offset = Number.parseInt(Buffer.from(cursor, "base64url").toString("utf8"), 10);
    return Number.isFinite(offset) && offset >= 0 ? offset : 0;
}
function encodeCursor(offset) {
    return Buffer.from(String(offset), "utf8").toString("base64url");
}
function firstString(value) {
    if (typeof value === "string" && value.trim())
        return value.trim();
    if (Array.isArray(value)) {
        for (const item of value) {
            const found = firstString(item);
            if (found)
                return found;
        }
    }
    if (value && typeof value === "object") {
        for (const item of Object.values(value)) {
            const found = firstString(item);
            if (found)
                return found;
        }
    }
    return undefined;
}
function asRecords(payload) {
    if (!payload || typeof payload !== "object")
        return [];
    const obj = payload;
    for (const key of ["records", "body", "results", "items"]) {
        const value = obj[key];
        if (Array.isArray(value))
            return value.filter((item) => !!item && typeof item === "object");
        if (value && typeof value === "object") {
            const nested = asRecords(value);
            if (nested.length)
                return nested;
        }
    }
    return [];
}
function totalCount(payload) {
    if (!payload || typeof payload !== "object")
        return undefined;
    const obj = payload;
    for (const key of ["total", "totalRecords", "count", "totalHits"]) {
        const value = obj[key];
        if (typeof value === "number")
            return value;
        if (typeof value === "string" && /^\d+$/.test(value))
            return Number(value);
    }
    for (const value of Object.values(obj)) {
        const nested = totalCount(value);
        if (nested !== undefined)
            return nested;
    }
    return undefined;
}
function digitalObjectUrl(record) {
    const text = JSON.stringify(record);
    const match = text.match(/https?:\/\/[^"\\]+(?:\.pdf|\.jpg|\.jpeg|\.png|\.tif|\.tiff)/i);
    return match?.[0];
}
function toSearchResult(record) {
    const id = firstString(record.naId) ??
        firstString(record.naIds) ??
        firstString(record.identifier) ??
        firstString(record.id) ??
        "unknown";
    const title = firstString(record.title) ??
        firstString(record.description) ??
        firstString(record.scopeAndContentNote) ??
        id;
    const url = `https://catalog.archives.gov/id/${encodeURIComponent(id)}`;
    const objectUrl = digitalObjectUrl(record);
    return {
        source: "nara",
        id,
        title,
        url,
        document_url: url,
        pdf_url: objectUrl?.toLowerCase().includes(".pdf") ? objectUrl : undefined,
        date: firstString(record.date) ?? firstString(record.inclusiveStartDate),
        collection: firstString(record.recordGroup) ?? firstString(record.collectionIdentifier),
        description: firstString(record.scopeAndContentNote) ?? firstString(record.description),
    };
}
export async function searchNaraCatalog(params) {
    const offset = decodeCursor(params.cursor);
    const url = new URL(`${params.base_url.replace(/\/$/, "")}/search`);
    url.searchParams.set("q", params.query);
    url.searchParams.set("limit", String(params.max_results));
    url.searchParams.set("offset", String(offset));
    if (params.available_online ?? true)
        url.searchParams.set("availableOnline", "true");
    const text = await fetchText(url.toString(), {
        source: "NARA Catalog",
        headers: { accept: "application/json" },
    });
    if (text.trimStart().startsWith("<")) {
        return {
            query: params.query,
            source: "nara_catalog",
            results: [],
            warnings: [
                "NARA returned HTML instead of JSON from the configured API endpoint. Use search_official_declass_sources for a manual Catalog URL, or update FOIA_SEARCH_NARA_API_BASE_URL.",
            ],
        };
    }
    const payload = JSON.parse(text);
    const records = asRecords(payload);
    const results = records.slice(0, params.max_results).map(toSearchResult);
    const total = totalCount(payload);
    const nextOffset = offset + results.length;
    return {
        query: params.query,
        source: "nara_catalog",
        results,
        next_cursor: total === undefined || nextOffset < total ? encodeCursor(nextOffset) : undefined,
        warnings: results.length === 0
            ? ["NARA API returned no records. Try broader keywords or available_online=false."]
            : undefined,
    };
}
export async function getNaraRecord(naid, base_url) {
    const url = new URL(`${base_url.replace(/\/$/, "")}/search`);
    url.searchParams.set("naIds", naid);
    url.searchParams.set("limit", "1");
    const text = await fetchText(url.toString(), {
        source: "NARA Catalog",
        headers: { accept: "application/json" },
    });
    if (text.trimStart().startsWith("<")) {
        return {
            source: "nara",
            id: naid,
            title: naid,
            url: `https://catalog.archives.gov/id/${encodeURIComponent(naid)}`,
            document_url: `https://catalog.archives.gov/id/${encodeURIComponent(naid)}`,
            metadata: {},
            citation_note: "NARA returned HTML instead of JSON from the configured API endpoint. Verify this NAID manually in the Catalog.",
        };
    }
    const payload = JSON.parse(text);
    const record = asRecords(payload)[0] ?? { naId: naid, title: naid };
    return {
        ...toSearchResult(record),
        metadata: record,
        text_preview: firstString(record.scopeAndContentNote) ?? firstString(record.description),
        citation_note: "National Archives Catalog metadata. Verify digitized object links and OCR/transcription at source.",
    };
}
