import type { StateSnapshot, IssueDetail, RefreshResponse } from './types';

const API_BASE = '/api/v1';

export async function fetchState(): Promise<StateSnapshot> {
    const res = await fetch(`${API_BASE}/state`);
    if (!res.ok) throw new Error(`Failed to fetch state: ${res.statusText}`);
    return res.json();
}

export async function fetchIssue(identifier: string): Promise<IssueDetail> {
    const res = await fetch(`${API_BASE}/${encodeURIComponent(identifier)}`);
    if (!res.ok) throw new Error(`Failed to fetch issue: ${res.statusText}`);
    return res.json();
}

export async function triggerRefresh(): Promise<RefreshResponse> {
    const res = await fetch(`${API_BASE}/refresh`, { method: 'POST' });
    if (!res.ok) throw new Error(`Failed to trigger refresh: ${res.statusText}`);
    return res.json();
}
