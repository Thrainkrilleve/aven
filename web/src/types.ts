export interface Env {
  DB: D1Database;
  ASSETS: Fetcher;
  AVEN_VERSION: string;
  REGISTRY_URL: string;
  ENVIRONMENT: string;
  REGISTRY_PUBLISH_TOKEN?: string;
}

export interface PackageRecord {
  id: number;
  name: string;
  version: string;
  description: string;
  authors: string;
  license: string;
  entry: string;
  sha256_seal: string;
  tarball_size_bytes: number;
  downloads: number;
  verified_units: string;
  created_at: string;
}

export interface SharedModelRecord {
  id: string;
  title: string;
  category: string;
  source_code: string;
  sha256: string;
  views: number;
  settled_facts_count: number;
  created_at: string;
}

export interface BenchmarkLogRecord {
  id: number;
  model_name: string;
  solve_time_us: number;
  facts_settled: number;
  platform: string;
  created_at: string;
}
