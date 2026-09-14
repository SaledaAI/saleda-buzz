import 'dart:convert';
import 'dart:io';

import 'package:buzz/shared/task_board/task_board_protocol.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('shared task-board projection fixtures match mobile', () async {
    final fixtureFile = [
      File('../crates/buzz-taskboard/fixtures/projection-v1.json'),
      File('crates/buzz-taskboard/fixtures/projection-v1.json'),
    ].firstWhere((candidate) => candidate.existsSync());
    final fixtures =
        jsonDecode(await fixtureFile.readAsString()) as Map<String, Object?>;
    expect(fixtures['version'], 1);
    for (final rawCase in fixtures['cases']! as List<Object?>) {
      final fixture = rawCase! as Map<String, Object?>;
      final projected = projectTaskBoardIssue(
        TaskBoardEvent.fromJson(fixture['issue']! as Map<String, Object?>),
        (fixture['statuses']! as List<Object?>)
            .map(
              (event) =>
                  TaskBoardEvent.fromJson(event! as Map<String, Object?>),
            )
            .toList(),
        maintainers: (fixture['maintainers']! as List<Object?>).cast<String>(),
      );
      expect(
        projected.toJson(),
        fixture['expected'],
        reason: fixture['name']! as String,
      );
    }
  });

  test('snapshot writer canonicalizes and stays provider neutral', () {
    final dependency = 'b' * 64;
    final snapshot = TaskBoardSnapshot(
      state: TaskBoardState.inProgress,
      dependencies: [dependency.toUpperCase(), dependency],
      baseRef: 'main',
      executor: 'e' * 64,
      automation: TaskAutomation.supervised,
    );
    final tags = snapshot.toTags();
    expect(tags.where((tag) => tag.first == taskDependsOnTag), hasLength(1));
    expect(tags.expand((tag) => tag), isNot(contains('provider')));
    expect(tags.expand((tag) => tag), isNot(contains('model')));
  });

  test('dependency analysis reports missing and cyclic tasks', () {
    final first = 'a' * 64;
    final second = 'b' * 64;
    final missing = 'c' * 64;
    ProjectedTaskBoardIssue projected(String id, List<String> dependencies) =>
        ProjectedTaskBoardIssue(
          issueId: id,
          snapshot: TaskBoardSnapshot(
            state: TaskBoardState.backlog,
            dependencies: dependencies,
            baseRef: null,
            executor: null,
            automation: TaskAutomation.supervised,
          ),
          statusEventId: null,
          updatedAt: 1,
          validationErrors: const [],
        );

    final result = analyzeTaskDependencies([
      projected(first, [second]),
      projected(second, [first, missing]),
    ]);
    expect(result.cycleMembers, {first, second});
    expect(result.missing, {
      second: [missing],
    });
  });

  test('code-task lifecycle ignores malformed newer updates', () {
    final issue = 'a' * 64;
    final requestId = 'b' * 64;
    final executor = 'c' * 64;
    final owner = 'd' * 64;
    final request = buildCodeTaskRequest(
      prompt: 'Implement the issue',
      baseRef: 'main',
    );
    final tags = buildCodeTaskRequestTags(
      issueId: issue,
      repoAddress: '30617:$owner:zorro',
      executor: executor,
    );
    TaskBoardEvent update(String id, int timestamp, String content) =>
        TaskBoardEvent(
          id: id,
          pubkey: executor,
          kind: 43003,
          createdAt: timestamp,
          tags: [
            ['e', requestId, '', 'root'],
            ['e', issue, '', 'issue'],
          ],
          content: content,
        );
    final projected = projectTaskJob(
      TaskBoardEvent(
        id: requestId,
        pubkey: owner,
        kind: 43001,
        createdAt: 1,
        tags: tags,
        content: jsonEncode(request.toJson()),
      ),
      [
        update('f' * 64, 3, 'not json'),
        update(
          'e' * 64,
          2,
          jsonEncode({'v': 1, 'phase': 'running', 'summary': 'Testing'}),
        ),
      ],
    );
    expect(projected.phase, TaskJobPhase.running);
    expect(projected.summary, 'Testing');
    expect(jsonEncode(request.toJson()), isNot(contains('provider')));
    expect(
      () => buildCodeTaskRequest(prompt: 'work', baseRef: '../main'),
      throwsFormatException,
    );
  });
}
