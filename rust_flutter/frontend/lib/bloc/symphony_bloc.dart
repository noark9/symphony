import 'dart:async';
import 'package:bloc/bloc.dart';
import '../api/api_client.dart';
import '../models/symphony_state.dart';

part 'symphony_event.dart';
part 'symphony_state_bloc.dart';

class SymphonyBloc extends Bloc<SymphonyEvent, SymphonyState> {
  final ApiClient apiClient;
  Timer? _pollingTimer;

  SymphonyBloc({required this.apiClient}) : super(SymphonyInitial()) {
    on<FetchState>(_onFetchState);
    on<RefreshRequested>(_onRefreshRequested);

    // Start auto-polling every 5 seconds
    add(FetchState());
    _startPolling();
  }

  void _startPolling() {
    _pollingTimer?.cancel();
    _pollingTimer = Timer.periodic(const Duration(seconds: 5), (_) {
      add(FetchState());
    });
  }

  Future<void> _onFetchState(
      FetchState event, Emitter<SymphonyState> emit) async {
    // Only emit loading state initially or on errors, to avoid UI flickering during polling
    if (state is SymphonyInitial || state is SymphonyError) {
      emit(SymphonyLoading());
    }

    try {
      final data = await apiClient.getState();
      emit(SymphonyLoaded(data));
    } catch (e) {
      emit(SymphonyError(e.toString()));
    }
  }

  Future<void> _onRefreshRequested(
      RefreshRequested event, Emitter<SymphonyState> emit) async {
    try {
      await apiClient.refresh();
      // Fetch immediately after triggering refresh
      add(FetchState());
    } catch (e) {
      emit(SymphonyError(e.toString()));
    }
  }

  @override
  Future<void> close() {
    _pollingTimer?.cancel();
    return super.close();
  }
}
