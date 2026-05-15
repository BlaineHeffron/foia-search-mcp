import type { DocumentDetail, SearchResponse } from "./types.js";
export interface CiaSearchParams {
    query: string;
    max_results: number;
    cursor?: string;
    base_url: string;
}
export declare function parseCiaSearch(html: string, base_url: string, query: string, page: number): SearchResponse;
export declare function searchCiaReadingRoom(params: CiaSearchParams): Promise<SearchResponse>;
export declare function parseCiaDocument(html: string, base_url: string, fallback_url: string): DocumentDetail;
export declare function getCiaDocument(id_or_url: string, base_url: string): Promise<DocumentDetail>;
