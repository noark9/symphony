part of 'symphony_bloc.dart';

abstract class SymphonyEvent {}

class FetchState extends SymphonyEvent {}

class RefreshRequested extends SymphonyEvent {}
