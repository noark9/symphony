import 'package:flutter/material.dart';
import '../../models/symphony_state.dart';

class RetryQueueList extends StatelessWidget {
  final List<RetryEntry> queue;

  const RetryQueueList({super.key, required this.queue});

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
              'Retry Queue (${queue.length})',
              style: Theme.of(context).textTheme.titleLarge?.copyWith(color: Colors.red.shade700),
            ),
            const Divider(),
            if (queue.isEmpty)
              const Padding(
                padding: EdgeInsets.all(16.0),
                child: Text('Retry queue is empty.'),
              )
            else
              ListView.separated(
                shrinkWrap: true,
                physics: const NeverScrollableScrollPhysics(),
                itemCount: queue.length,
                separatorBuilder: (context, index) => const Divider(),
                itemBuilder: (context, index) {
                  final retry = queue[index];
                  final now = DateTime.now();
                  final dueIn = retry.nextRetryAt.difference(now);
                  final isDueNow = dueIn.isNegative;

                  return ListTile(
                    leading: const Icon(Icons.warning, color: Colors.orange),
                    title: Text(retry.issueId, style: const TextStyle(fontWeight: FontWeight.bold)),
                    subtitle: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('Attempt: ${retry.attemptCount}'),
                        Text(
                          isDueNow ? 'Due now' : 'Due in: ${_formatDuration(dueIn)}',
                          style: TextStyle(color: isDueNow ? Colors.red : Colors.grey.shade700),
                        ),
                      ],
                    ),
                  );
                },
              ),
          ],
        ),
      ),
    );
  }

  String _formatDuration(Duration duration) {
    String twoDigits(int n) => n.toString().padLeft(2, "0");
    String twoDigitMinutes = twoDigits(duration.inMinutes.remainder(60));
    String twoDigitSeconds = twoDigits(duration.inSeconds.remainder(60));
    if (duration.inHours > 0) {
      return "${twoDigits(duration.inHours)}:$twoDigitMinutes:$twoDigitSeconds";
    }
    return "$twoDigitMinutes:$twoDigitSeconds";
  }
}
