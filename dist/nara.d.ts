import type { DocumentDetail, SearchResponse } from "./types.js";
export interface NaraSearchParams {
    query: string;
    max_results: number;
    cursor?: string;
    available_online?: boolean;
    base_url: string;
}
export declare function searchNaraCatalog(params: NaraSearchParams): Promise<SearchResponse>;
export declare function getNaraRecord(naid: string, base_url: string): Promise<DocumentDetail>;
