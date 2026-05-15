import type { SearchResult } from "./types.js";
export interface OfficialSourceSpec {
    source: string;
    title: string;
    url: string;
    search_url_template: string;
    best_for: string;
    caveats: string;
}
export declare const OFFICIAL_SOURCES: OfficialSourceSpec[];
export declare function searchOfficialSources(query: string, source_filter?: string[]): SearchResult[];
