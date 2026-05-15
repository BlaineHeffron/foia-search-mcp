import type { SearchResult } from "./types.js";

export interface OfficialSourceSpec {
  source: string;
  title: string;
  url: string;
  search_url_template: string;
  best_for: string;
  caveats: string;
}

export const OFFICIAL_SOURCES: OfficialSourceSpec[] = [
  {
    source: "cia",
    title: "CIA FOIA Electronic Reading Room",
    url: "https://www.cia.gov/readingroom/",
    search_url_template: "https://www.cia.gov/readingroom/search/site/{query}",
    best_for: "CIA CREST/FOIA documents, historical intelligence reports, declassified memos.",
    caveats: "OCR can be poor; document pages may omit full scan text. Verify PDF scans.",
  },
  {
    source: "nara",
    title: "National Archives Catalog",
    url: "https://catalog.archives.gov/",
    search_url_template: "https://catalog.archives.gov/search?q={query}",
    best_for: "Archival descriptions, record groups, digitized federal records, declassified holdings.",
    caveats: "API/search shape changes; many records are descriptive only and not digitized.",
  },
  {
    source: "frus",
    title: "Foreign Relations of the United States",
    url: "https://history.state.gov/historicaldocuments",
    search_url_template: "https://history.state.gov/search?q={query}",
    best_for: "State Department documentary history, including Operation Popeye and Vietnam-era policy docs.",
    caveats: "Curated volumes, not general FOIA. Good citations, limited to published FRUS selections.",
  },
  {
    source: "dtic",
    title: "Defense Technical Information Center Public Search",
    url: "https://discover.dtic.mil/",
    search_url_template: "https://discover.dtic.mil/results/?q={query}",
    best_for: "Unclassified DoD technical reports, defense research, plasma/ionosphere/weather-mod reports.",
    caveats: "Public interface has limited stable API behavior. Some records require manual download.",
  },
  {
    source: "govinfo",
    title: "GovInfo",
    url: "https://www.govinfo.gov/",
    search_url_template: "https://www.govinfo.gov/app/search/{query}",
    best_for: "Congressional hearings, CFR, treaty materials, government reports.",
    caveats: "Best for known hearings/statutes; broad search can be noisy.",
  },
  {
    source: "noaa",
    title: "NOAA Institutional Repository and Weather Modification Reports",
    url: "https://repository.library.noaa.gov/",
    search_url_template: "https://repository.library.noaa.gov/search?query={query}",
    best_for: "Project Stormfury, NOAA technical reports, weather-modification reporting collections.",
    caveats: "Repository search varies by collection; cross-check NOAA Library project report pages.",
  },
];

export function searchOfficialSources(query: string, source_filter?: string[]): SearchResult[] {
  const q = query.toLowerCase();
  const filters = new Set((source_filter ?? []).map((item) => item.toLowerCase()));
  return OFFICIAL_SOURCES.filter((source) => {
    if (filters.size && !filters.has(source.source)) return false;
    const haystack = `${source.source} ${source.title} ${source.best_for} ${source.caveats}`.toLowerCase();
    return q
      .split(/\s+/)
      .filter(Boolean)
      .some((term) => haystack.includes(term));
  }).map((source) => ({
    source: source.source,
    id: source.source,
    title: source.title,
    url: source.url,
    document_url: source.search_url_template.replace("{query}", encodeURIComponent(query)),
    description: `Best for: ${source.best_for} Caveats: ${source.caveats}`,
  }));
}
