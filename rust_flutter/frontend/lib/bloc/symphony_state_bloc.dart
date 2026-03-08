part of 'symphony_bloc.dart';

abstract class SymphonyState {}

class SymphonyInitial extends SymphonyState {}

class SymphonyLoading extends SymphonyState {}

class SymphonyLoaded extends SymphonyState {
  final SymphonyStateData data;

  SymphonyLoaded(this.data);
}

class SymphonyError extends SymphonyState {
  final String message;

  SymphonyError(this.message);
}
