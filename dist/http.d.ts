export interface FetchTextOptions {
    source: string;
    timeout_ms?: number;
    headers?: Record<string, string>;
}
export declare function fetchText(url: string, options: FetchTextOptions): Promise<string>;
export declare function absolutize(url: string, base: string): string;
