import 'package:flutter/material.dart';
import '../../models/symphony_state.dart';

class RunningSessionsList extends StatelessWidget {
  final List<RunningSession> sessions;

  const RunningSessionsList({super.key, required this.sessions});

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
              'Running Sessions (${sessions.length})',
              style: Theme.of(context).textTheme.titleLarge,
            ),
            const Divider(),
            if (sessions.isEmpty)
              const Padding(
                padding: EdgeInsets.all(16.0),
                child: Text('No running sessions.'),
              )
            else
              ListView.separated(
                shrinkWrap: true,
                physics: const NeverScrollableScrollPhysics(),
                itemCount: sessions.length,
                separatorBuilder: (context, index) => const Divider(),
                itemBuilder: (context, index) {
                  final session = sessions[index];
                  final elapsed = DateTime.now().difference(session.startedAt);
                  final timeSinceLastEvent = DateTime.now().difference(session.lastHeartbeat);

                  return ListTile(
                    leading: const Icon(Icons.play_circle_fill, color: Colors.blue),
                    title: Text(session.issueId, style: const TextStyle(fontWeight: FontWeight.bold)),
                    subtitle: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('Elapsed: ${_formatDuration(elapsed)}'),
                        Text('Last event: ${_formatDuration(timeSinceLastEvent)} ago'),
                        const Text('Turn count: N/A', style: TextStyle(color: Colors.grey)), // Turn count is not in API currently
                      ],
                    ),
                    isThreeLine: true,
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
