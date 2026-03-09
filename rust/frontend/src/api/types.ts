/// Symphony API types — mirrors Rust orchestrator::StateSnapshot
export interface StateSnapshot {
    generated_at: string;
    counts: {
        running: number;
        retrying: number;
    };
    running: RunningSession[];
    retrying: RetryEntry[];
    gemini_totals: GeminiTotals;
    rate_limits: Record<string, unknown> | null;
}

export interface RunningSession {
    issue_id: string;
    issue_identifier: string;
    state: string;
    session_id: string | null;
    turn_count: number;
    last_event: string | null;
    last_message: string | null;
    started_at: string;
    last_event_at: string | null;
    tokens: TokensSnapshot;
}

export interface TokensSnapshot {
    input_tokens: number;
    output_tokens: number;
    total_tokens: number;
}

export interface RetryEntry {
    issue_id: string;
    issue_identifier: string;
    attempt: number;
    due_at: string;
    error: string | null;
}

export interface GeminiTotals {
    input_tokens: number;
    output_tokens: number;
    total_tokens: number;
    seconds_running: number;
}

export interface IssueDetail {
    issue_identifier: string;
    issue_id: string;
    status: string;
    workspace: { path: string };
    running: RunningSession | null;
    retry: RetryEntry | null;
    last_error: string | null;
}

export interface RefreshResponse {
    queued: boolean;
    coalesced: boolean;
    requested_at: string;
    operations: string[];
}
