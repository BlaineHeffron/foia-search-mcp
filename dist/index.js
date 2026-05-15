#!/usr/bin/env node
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { getCiaDocument, searchCiaReadingRoom } from "./cia.js";
import { loadConfig } from "./config.js";
import { getNaraRecord, searchNaraCatalog } from "./nara.js";
import { searchOfficialSources } from "./officialSources.js";
import { SourceError } from "./types.js";
const config = loadConfig();
function maxResults(value) {
    return Math.max(1, Math.min(value ?? 10, config.max_results_cap));
}
function text(payload) {
    return {
        content: [
            {
                type: "text",
                text: JSON.stringify(payload, null, 2),
            },
        ],
    };
}
function toToolError(error) {
    if (error instanceof SourceError) {
        return text({
            error: error.message,
            source: error.source,
            status: error.status,
            next_action: error.action,
        });
    }
    const message = error instanceof Error ? error.message : String(error);
    return text({ error: message, next_action: "Retry with narrower terms or another source." });
}
export function createServer() {
    const server = new McpServer({
        name: "foia-search",
        version: "0.1.0",
    });
    server.tool("search_cia_reading_room", "Search CIA FOIA Electronic Reading Room pages for declassified documents. Use for CIA CREST/FOIA records, historical intelligence memos, and weather/plasma/ionosphere terms. This scrapes public search HTML, so verify important hits against source PDFs.", {
        query: z.string().min(2).describe("CIA Reading Room query. Boolean operators like AND/OR/NOT may work on source site."),
        max_results: z.number().int().min(1).max(25).optional().describe("Maximum hits to return. Default 10, cap 25."),
        cursor: z.string().optional().describe("Opaque cursor from prior response for next page."),
    }, async ({ query, max_results, cursor }) => {
        try {
            return text(await searchCiaReadingRoom({
                query,
                max_results: maxResults(max_results),
                cursor,
                base_url: config.cia_base_url,
            }));
        }
        catch (error) {
            return toToolError(error);
        }
    });
    server.tool("get_cia_document", "Fetch metadata and source links for one CIA Reading Room document. Use after search_cia_reading_room returns an id, or when you already have a /readingroom/document/ URL. Output includes scan/PDF links when the page exposes them; OCR may be incomplete.", {
        id_or_url: z.string().min(3).describe("CIA document id like cia-rdp68r00530a000200110020-2, or full Reading Room document URL."),
    }, async ({ id_or_url }) => {
        try {
            return text(await getCiaDocument(id_or_url, config.cia_base_url));
        }
        catch (error) {
            return toToolError(error);
        }
    });
    server.tool("search_nara_catalog", "Search National Archives Catalog metadata for federal/declassified records. Use for archival record groups, digitized holdings, and official metadata beyond CIA. The Catalog API can be inconsistent; use broader terms and available_online=false if needed.", {
        query: z.string().min(2).describe("Catalog query, e.g. weather modification, ionosphere, Project Skywater."),
        max_results: z.number().int().min(1).max(25).optional().describe("Maximum records to return. Default 10, cap 25."),
        cursor: z.string().optional().describe("Opaque cursor from prior response for next page."),
        available_online: z.boolean().optional().describe("Limit to records with online objects. Default true."),
    }, async ({ query, max_results, cursor, available_online }) => {
        try {
            return text(await searchNaraCatalog({
                query,
                max_results: maxResults(max_results),
                cursor,
                available_online,
                base_url: config.nara_api_base_url,
            }));
        }
        catch (error) {
            return toToolError(error);
        }
    });
    server.tool("get_nara_record", "Fetch one National Archives Catalog record by NAID. Use after search_nara_catalog when you need complete metadata and digitized-object links. Verify scans/OCR at the Catalog URL before citing.", {
        naid: z.string().min(1).describe("National Archives Identifier (NAID)."),
    }, async ({ naid }) => {
        try {
            return text(await getNaraRecord(naid, config.nara_api_base_url));
        }
        catch (error) {
            return toToolError(error);
        }
    });
    server.tool("search_official_declass_sources", "Return official/public-source search entry points suited to a declassified-doc research query. Use when deciding where to search next across CIA, NARA, FRUS, DTIC, GovInfo, and NOAA. This does not search all documents; it gives source-specific search URLs and caveats.", {
        query: z.string().min(2).describe("Research topic, e.g. Operation Popeye ionosphere heating weather modification."),
        sources: z.array(z.string()).optional().describe("Optional source filter: cia, nara, frus, dtic, govinfo, noaa."),
    }, async ({ query, sources }) => text({ query, results: searchOfficialSources(query, sources) }));
    return server;
}
async function main() {
    const transport = new StdioServerTransport();
    await createServer().connect(transport);
}
main().catch((error) => {
    console.error(error);
    process.exit(1);
});
