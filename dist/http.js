import { SourceError } from "./types.js";
const DEFAULT_TIMEOUT_MS = 30_000;
export async function fetchText(url, options) {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), options.timeout_ms ?? DEFAULT_TIMEOUT_MS);
    try {
        const response = await fetch(url, {
            signal: controller.signal,
            headers: {
                "user-agent": "foia-search-mcp/0.1 (+local research tool; contact: local-user)",
                accept: "text/html,application/json,application/pdf;q=0.9,*/*;q=0.8",
                ...options.headers,
            },
        });
        if (!response.ok) {
            throw new SourceError(`${options.source} returned HTTP ${response.status} for ${url}`, options.source, response.status, response.status === 429 ? "Retry later or narrow the query." : "Check source availability or query syntax.");
        }
        return await response.text();
    }
    catch (error) {
        if (error instanceof SourceError)
            throw error;
        const message = error instanceof Error ? error.message : String(error);
        throw new SourceError(`${options.source} request failed for ${url}: ${message}`, options.source, undefined, "Retry, narrow the query, or use another official source.");
    }
    finally {
        clearTimeout(timeout);
    }
}
export function absolutize(url, base) {
    return new URL(url, base).toString();
}
