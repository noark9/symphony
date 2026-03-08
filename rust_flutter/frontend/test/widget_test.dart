import 'package:flutter_test/flutter_test.dart';
import 'package:frontend/main.dart';
import 'package:frontend/api/api_client.dart';

void main() {
  testWidgets('Dashboard renders', (WidgetTester tester) async {
    final apiClient = ApiClient();
    await tester.pumpWidget(MyApp(apiClient: apiClient));

    expect(find.text('Symphony Dashboard'), findsOneWidget);
  });
}
