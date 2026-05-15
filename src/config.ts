export interface Config {
  cia_base_url: string;
  nara_api_base_url: string;
  max_results_cap: number;
}

export function loadConfig(): Config {
  return {
    cia_base_url: process.env.FOIA_SEARCH_CIA_BASE_URL ?? "https://www.cia.gov",
    nara_api_base_url:
      process.env.FOIA_SEARCH_NARA_API_BASE_URL ??
      "https://catalog.archives.gov/api/v2/records",
    max_results_cap: Number(process.env.FOIA_SEARCH_MAX_RESULTS ?? "25"),
  };
}
