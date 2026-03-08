import 'package:flutter/material.dart';
import '../../models/symphony_state.dart';

class SummaryCard extends StatelessWidget {
  final SymphonyStateData data;

  const SummaryCard({super.key, required this.data});

  @override
  Widget build(BuildContext context) {
    return Card(
      elevation: 4,
      child: Padding(
        padding: const EdgeInsets.all(16.0),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'System Overview',
              style: Theme.of(context).textTheme.titleLarge,
            ),
            const Divider(),
            const SizedBox(height: 8),
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceAround,
              children: [
                _buildStatColumn(context, 'Running', data.counts.running.toString(), Colors.blue),
                _buildStatColumn(context, 'Claimed', data.counts.claimed.toString(), Colors.orange),
                _buildStatColumn(context, 'Retries', data.counts.retries.toString(), Colors.red),
                Container(width: 1, height: 40, color: Colors.grey.shade300),
                _buildStatColumn(context, 'Prompt Tokens', data.geminiTotals.promptTokens.toString(), Colors.green),
                _buildStatColumn(context, 'Candidate Tokens', data.geminiTotals.candidateTokens.toString(), Colors.green),
                _buildStatColumn(context, 'Total Requests', data.geminiTotals.totalRequests.toString(), Colors.purple),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildStatColumn(BuildContext context, String label, String value, Color color) {
    return Column(
      children: [
        Text(
          value,
          style: Theme.of(context).textTheme.headlineMedium?.copyWith(
                color: color,
                fontWeight: FontWeight.bold,
              ),
        ),
        const SizedBox(height: 4),
        Text(
          label,
          style: Theme.of(context).textTheme.bodySmall,
        ),
      ],
    );
  }
}
