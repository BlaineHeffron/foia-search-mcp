export type SourceName = "cia" | "nara" | "frus" | "dtic" | "govinfo" | "noaa";
export interface SearchResult {
    source: SourceName | string;
    id: string;
    title: string;
    url: string;
    date?: string;
    collection?: string;
    description?: string;
    document_url?: string;
    pdf_url?: string;
    score?: number;
}
export interface DocumentDetail extends SearchResult {
    metadata: Record<string, unknown>;
    text_preview?: string;
    attachments?: Array<{
        label: string;
        url: string;
        type?: string;
    }>;
    citation_note?: string;
}
export interface SearchResponse {
    query: string;
    source: string;
    results: SearchResult[];
    next_cursor?: string;
    warnings?: string[];
}
export declare class SourceError extends Error {
    readonly source: string;
    readonly status?: number | undefined;
    readonly action?: string | undefined;
    constructor(message: string, source: string, status?: number | undefined, action?: string | undefined);
}
