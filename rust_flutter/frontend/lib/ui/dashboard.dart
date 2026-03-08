import 'package:flutter/material.dart';
import 'package:flutter_bloc/flutter_bloc.dart';
import '../bloc/symphony_bloc.dart';
import 'widgets/summary_card.dart';
import 'widgets/running_sessions_list.dart';
import 'widgets/retry_queue_list.dart';

class Dashboard extends StatelessWidget {
  const Dashboard({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Symphony Dashboard'),
        actions: [
          IconButton(
            icon: const Icon(Icons.refresh),
            onPressed: () {
              context.read<SymphonyBloc>().add(RefreshRequested());
            },
            tooltip: 'Manual Refresh',
          ),
        ],
      ),
      body: BlocBuilder<SymphonyBloc, SymphonyState>(
        builder: (context, state) {
          if (state is SymphonyInitial || state is SymphonyLoading) {
            return const Center(child: CircularProgressIndicator());
          } else if (state is SymphonyError) {
            return Center(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  const Icon(Icons.error_outline, color: Colors.red, size: 60),
                  const SizedBox(height: 16),
                  Text('Error: ${state.message}', textAlign: TextAlign.center),
                  const SizedBox(height: 16),
                  ElevatedButton(
                    onPressed: () {
                      context.read<SymphonyBloc>().add(FetchState());
                    },
                    child: const Text('Retry'),
                  ),
                ],
              ),
            );
          } else if (state is SymphonyLoaded) {
            final data = state.data;
            return SingleChildScrollView(
              padding: const EdgeInsets.all(16.0),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  SummaryCard(data: data),
                  const SizedBox(height: 24),
                  Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Expanded(
                        child: RunningSessionsList(sessions: data.runningSessions),
                      ),
                      const SizedBox(width: 24),
                      Expanded(
                        child: RetryQueueList(queue: data.retryQueue),
                      ),
                    ],
                  ),
                ],
              ),
            );
          }

          return const SizedBox.shrink();
        },
      ),
    );
  }
}
