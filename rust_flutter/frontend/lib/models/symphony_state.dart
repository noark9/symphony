class SymphonyStateData {
  final Counts counts;
  final List<RunningSession> runningSessions;
  final List<RetryEntry> retryQueue;
  final GeminiTotals geminiTotals;

  SymphonyStateData({
    required this.counts,
    required this.runningSessions,
    required this.retryQueue,
    required this.geminiTotals,
  });

  factory SymphonyStateData.fromJson(Map<String, dynamic> json) {
    return SymphonyStateData(
      counts: Counts.fromJson(json['counts']),
      runningSessions: (json['running_sessions'] as List)
          .map((e) => RunningSession.fromJson(e))
          .toList(),
      retryQueue: (json['retry_queue'] as List)
          .map((e) => RetryEntry.fromJson(e))
          .toList(),
      geminiTotals: GeminiTotals.fromJson(json['gemini_totals']),
    );
  }
}

class Counts {
  final int running;
  final int claimed;
  final int retries;

  Counts({
    required this.running,
    required this.claimed,
    required this.retries,
  });

  factory Counts.fromJson(Map<String, dynamic> json) {
    return Counts(
      running: json['running'] as int,
      claimed: json['claimed'] as int,
      retries: json['retries'] as int,
    );
  }
}

class RunningSession {
  final String issueId;
  final DateTime startedAt;
  final DateTime lastHeartbeat;

  RunningSession({
    required this.issueId,
    required this.startedAt,
    required this.lastHeartbeat,
  });

  factory RunningSession.fromJson(Map<String, dynamic> json) {
    return RunningSession(
      issueId: json['issue_id'] as String,
      startedAt: DateTime.parse(json['started_at'] as String),
      lastHeartbeat: DateTime.parse(json['last_heartbeat'] as String),
    );
  }
}

class RetryEntry {
  final String issueId;
  final int attemptCount;
  final DateTime nextRetryAt;

  RetryEntry({
    required this.issueId,
    required this.attemptCount,
    required this.nextRetryAt,
  });

  factory RetryEntry.fromJson(Map<String, dynamic> json) {
    return RetryEntry(
      issueId: json['issue_id'] as String,
      attemptCount: json['attempt_count'] as int,
      nextRetryAt: DateTime.parse(json['next_retry_at'] as String),
    );
  }
}

class GeminiTotals {
  final int promptTokens;
  final int candidateTokens;
  final int totalRequests;

  GeminiTotals({
    required this.promptTokens,
    required this.candidateTokens,
    required this.totalRequests,
  });

  factory GeminiTotals.fromJson(Map<String, dynamic> json) {
    return GeminiTotals(
      promptTokens: json['prompt_tokens'] as int,
      candidateTokens: json['candidate_tokens'] as int,
      totalRequests: json['total_requests'] as int,
    );
  }
}
