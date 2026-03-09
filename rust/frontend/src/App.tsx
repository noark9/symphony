import { useState, useEffect } from 'react';
import { QueryClient, QueryClientProvider, useQuery, useMutation } from '@tanstack/react-query';
import { fetchState, triggerRefresh } from './api/client';
import type { StateSnapshot, RunningSession, RetryEntry } from './api/types';
import './index.css';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchInterval: 2000,
      retry: 2,
      staleTime: 1000,
    },
  },
});

type Theme = 'dark' | 'light';

function useTheme(): [Theme, () => void] {
  const [theme, setTheme] = useState<Theme>(() => {
    const saved = localStorage.getItem('symphony-theme');
    if (saved === 'light' || saved === 'dark') return saved;
    // Respect system preference
    if (window.matchMedia('(prefers-color-scheme: light)').matches) return 'light';
    return 'dark';
  });

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
    localStorage.setItem('symphony-theme', theme);
  }, [theme]);

  const toggle = () => setTheme((t) => (t === 'dark' ? 'light' : 'dark'));
  return [theme, toggle];
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toLocaleString();
}

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60)}s`;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

function formatTime(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString();
  } catch {
    return iso;
  }
}

function formatRelative(iso: string): string {
  try {
    const diff = Date.now() - new Date(iso).getTime();
    if (diff < 60_000) return `${Math.round(diff / 1000)}s ago`;
    if (diff < 3600_000) return `${Math.floor(diff / 60_000)}m ago`;
    return `${Math.floor(diff / 3600_000)}h ago`;
  } catch {
    return iso;
  }
}

function Dashboard() {
  const { data, error, isLoading, dataUpdatedAt } = useQuery<StateSnapshot>({
    queryKey: ['state'],
    queryFn: fetchState,
  });

  if (isLoading) {
    return (
      <div className="loading">
        <div className="loading-spinner" />
        Connecting to Symphony...
      </div>
    );
  }

  if (error) {
    return (
      <div className="error-banner">
        ⚠️ Failed to connect: {(error as Error).message}
      </div>
    );
  }

  if (!data) return null;

  return (
    <>
      {/* Stats Overview */}
      <div className="stats-grid">
        <div className="stat-card">
          <div className="stat-label">Running</div>
          <div className={`stat-value ${data.counts.running > 0 ? 'green' : ''}`}>
            {data.counts.running}
          </div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Retrying</div>
          <div className={`stat-value ${data.counts.retrying > 0 ? 'yellow' : ''}`}>
            {data.counts.retrying}
          </div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Total Tokens</div>
          <div className="stat-value blue">
            {formatTokens(data.gemini_totals.total_tokens)}
          </div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Runtime</div>
          <div className="stat-value purple">
            {formatDuration(data.gemini_totals.seconds_running)}
          </div>
        </div>
      </div>

      {/* Token Breakdown */}
      <div className="stats-grid" style={{ marginBottom: 24 }}>
        <div className="stat-card">
          <div className="stat-label">Input Tokens</div>
          <div className="stat-value">{formatTokens(data.gemini_totals.input_tokens)}</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Output Tokens</div>
          <div className="stat-value">{formatTokens(data.gemini_totals.output_tokens)}</div>
        </div>
      </div>

      {/* Running Sessions */}
      <div className="section">
        <div className="section-header">
          <span className="section-title">Running Sessions</span>
          <span className="section-badge green">{data.counts.running}</span>
        </div>
        {data.running.length > 0 ? (
          <div className="table-container">
            <table>
              <thead>
                <tr>
                  <th>Issue</th>
                  <th>State</th>
                  <th>Turns</th>
                  <th>Tokens</th>
                  <th>Last Event</th>
                  <th>Started</th>
                  <th>Last Activity</th>
                </tr>
              </thead>
              <tbody>
                {data.running.map((r: RunningSession) => (
                  <tr key={r.issue_id}>
                    <td className="issue-id">{r.issue_identifier}</td>
                    <td><span className="state-badge active">{r.state}</span></td>
                    <td className="mono">{r.turn_count}</td>
                    <td className="mono">{formatTokens(r.tokens.total_tokens)}</td>
                    <td className="truncate">{r.last_event ?? '—'}</td>
                    <td className="mono">{formatTime(r.started_at)}</td>
                    <td className="mono">{r.last_event_at ? formatRelative(r.last_event_at) : '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="empty-state">
            <span className="emoji">💤</span>
            No active sessions
          </div>
        )}
      </div>

      {/* Retry Queue */}
      <div className="section">
        <div className="section-header">
          <span className="section-title">Retry Queue</span>
          <span className="section-badge yellow">{data.counts.retrying}</span>
        </div>
        {data.retrying.length > 0 ? (
          <div className="table-container">
            <table>
              <thead>
                <tr>
                  <th>Issue</th>
                  <th>Attempt</th>
                  <th>Due At</th>
                  <th>Error</th>
                </tr>
              </thead>
              <tbody>
                {data.retrying.map((r: RetryEntry) => (
                  <tr key={r.issue_id}>
                    <td className="issue-id">{r.issue_identifier}</td>
                    <td><span className="state-badge retrying">#{r.attempt}</span></td>
                    <td className="mono">{formatTime(r.due_at)}</td>
                    <td className="error-cell">{r.error ?? '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="empty-state">
            <span className="emoji">✅</span>
            No pending retries
          </div>
        )}
      </div>

      {/* Footer */}
      <div style={{ textAlign: 'center', padding: '16px 0', color: 'var(--text-muted)', fontSize: 12 }}>
        Last updated: {dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleTimeString() : '—'}
        &nbsp;·&nbsp;
        Auto-refreshing every 2s
      </div>
    </>
  );
}

function App() {
  const [theme, toggleTheme] = useTheme();

  return (
    <QueryClientProvider client={queryClient}>
      <div className="app">
        <header className="header">
          <div className="header-left">
            <span className="logo">⚡</span>
            <h1>Symphony</h1>
          </div>
          <div className="header-right">
            <div className="status-dot connected" title="Connected" />
            <button
              className="theme-toggle"
              onClick={toggleTheme}
              title={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
            >
              {theme === 'dark' ? '☀️' : '🌙'}
            </button>
            <DashboardRefreshButton />
          </div>
        </header>
        <Dashboard />
      </div>
    </QueryClientProvider>
  );
}

function DashboardRefreshButton() {
  const refreshMutation = useMutation({
    mutationFn: triggerRefresh,
    onSuccess: () => {
      setTimeout(() => queryClient.invalidateQueries({ queryKey: ['state'] }), 500);
    },
  });

  return (
    <button
      className={`refresh-btn ${refreshMutation.isPending ? 'spinning' : ''}`}
      onClick={() => refreshMutation.mutate()}
      disabled={refreshMutation.isPending}
    >
      <span className="icon">⟳</span>
      Refresh
    </button>
  );
}

export default App;
