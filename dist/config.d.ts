export interface Config {
    cia_base_url: string;
    nara_api_base_url: string;
    max_results_cap: number;
}
export declare function loadConfig(): Config;
