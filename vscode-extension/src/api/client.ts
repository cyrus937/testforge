/**
 * HTTP client for the TestForge API server.
 *
 * All methods throw on network errors or non-2xx responses,
 * with human-readable error messages.
 */

import axios, { AxiosInstance, AxiosError } from 'axios';

export interface HealthResponse {
    status: string;
    version: string;
    index_ready: boolean;
    uptime_seconds: number;
}

export interface StatusResponse {
    file_count: number;
    symbol_count: number;
    embedding_count: number;
    languages: string[];
    last_indexed: string | null;
    watcher_active: boolean;
    vector_count: number;
    text_doc_count: number;
}

export interface SearchResult {
    symbol: SymbolInfo;
    score: number;
    match_source: string;
}

export interface SymbolInfo {
    name: string;
    qualified_name: string;
    kind: string;
    language: string;
    file_path: string;
    start_line: number;
    end_line: number;
    signature?: string;
    docstring?: string;
    dependencies: string[];
    parent?: string;
    visibility: string;
}

export interface SearchResponse {
    results: SearchResult[];
    total_results: number;
    search_time_ms: number;
}

export interface IndexJobResponse {
    job_id: string;
    status: string;
    progress_ws: string;
}

export interface GenerateJobResponse {
    job_id: string;
    status: string;
    target: string;
}

export interface GenerateResultResponse {
    job_id: string;
    status: string;
    result?: {
        source: string;
        file_name: string;
        target_symbol: string;
        test_count: number;
        framework: string;
        warnings: string[];
    };
    error?: string;
}

export class ApiClient {
    private http: AxiosInstance;
    private baseUrl: string;

    constructor(baseUrl: string) {
        this.baseUrl = baseUrl.replace(/\/$/, '');
        this.http = axios.create({
            baseURL: `${this.baseUrl}/api`,
            timeout: 30000,
            headers: { 'Content-Type': 'application/json' },
        });
    }

    /** Check server health. */
    async getHealth(): Promise<HealthResponse> {
        const { data } = await this.http.get<HealthResponse>('/health');
        return data;
    }

    /** Get index statistics. */
    async getStatus(): Promise<StatusResponse> {
        const { data } = await this.http.get<StatusResponse>('/status');
        return data;
    }

    /** Search the codebase. */
    async search(
        query: string,
        limit: number = 10,
        filters?: {
            languages?: string[];
            kinds?: string[];
            paths?: string[];
        }
    ): Promise<SearchResponse> {
        const { data } = await this.http.post<SearchResponse>('/search', {
            query,
            limit,
            filters: filters || {},
        });
        return data;
    }

    /** Trigger project indexing. */
    async triggerIndex(clean: boolean = false): Promise<IndexJobResponse> {
        const { data } = await this.http.post<IndexJobResponse>('/index', {
            path: '.',
            clean,
        });
        return data;
    }

    /** Request test generation for a symbol. */
    async generateTests(
        target: string,
        options?: {
            framework?: string;
            include_edge_cases?: boolean;
            include_mocks?: boolean;
        }
    ): Promise<GenerateJobResponse> {
        const { data } = await this.http.post<GenerateJobResponse>('/generate-tests', {
            target,
            framework: options?.framework || 'pytest',
            include_edge_cases: options?.include_edge_cases ?? true,
            include_mocks: options?.include_mocks ?? true,
        });
        return data;
    }

    /** Poll for generation result. */
    async getGenerationResult(jobId: string): Promise<GenerateResultResponse> {
        const { data } = await this.http.get<GenerateResultResponse>(
            `/generate-tests/${jobId}`
        );
        return data;
    }

    /** List symbols with optional filters. */
    async listSymbols(params?: {
        file?: string;
        kind?: string;
        language?: string;
        limit?: number;
    }): Promise<{ symbols: SymbolInfo[]; total: number }> {
        const { data } = await this.http.get('/symbols', { params });
        return data;
    }

    /** Get details for a specific symbol. */
    async getSymbol(name: string): Promise<{
        symbol: SymbolInfo;
        context: { dependencies: string[]; callers: string[]; siblings: string[] };
    }> {
        const { data } = await this.http.get(`/symbols/${encodeURIComponent(name)}`);
        return data;
    }

    /** Get the WebSocket URL for a job. */
    getProgressWsUrl(jobId: string): string {
        const wsBase = this.baseUrl.replace(/^http/, 'ws');
        return `${wsBase}/ws/progress/${jobId}`;
    }

    /** Check if the server is reachable. */
    async isAvailable(): Promise<boolean> {
        try {
            await this.getHealth();
            return true;
        } catch {
            return false;
        }
    }
}