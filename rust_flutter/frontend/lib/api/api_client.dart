import 'dart:convert';
import 'package:http/http.dart' as http;
import '../models/symphony_state.dart';

class ApiClient {
  final String baseUrl;

  ApiClient({this.baseUrl = 'http://localhost:3000/api/v1'});

  Future<SymphonyStateData> getState() async {
    final response = await http.get(Uri.parse('$baseUrl/state'));

    if (response.statusCode == 200) {
      return SymphonyStateData.fromJson(json.decode(response.body));
    } else {
      throw Exception('Failed to load state: ${response.statusCode}');
    }
  }

  Future<void> refresh() async {
    final response = await http.post(Uri.parse('$baseUrl/refresh'));

    if (response.statusCode != 200) {
      throw Exception('Failed to trigger refresh: ${response.statusCode}');
    }
  }
}
