/// Provider-neutral task-board event projection shared by mobile features.
library;

import 'dart:convert';

/// Compile-time preview gate shared by the mobile task-board route.
const taskBoardPreviewEnabled = bool.fromEnvironment(
  'ZORRO_TASK_BOARD_PREVIEW',
  defaultValue: false,
);

const taskBoardStateTag = 'buzz-board-state';
const taskDependsOnTag = 'buzz-depends-on';
const taskBaseRefTag = 'buzz-base-ref';
const taskExecutorTag = 'buzz-executor';
const taskAutomationTag = 'buzz-automation';

final _hex64 = RegExp(r'^[0-9a-fA-F]{64}$');
const _statusKinds = {1630, 1631, 1632, 1633};
const _jobUpdateKinds = {43002, 43003, 43004, 43006};

/// Fine-grained board column projected over NIP-34 status kinds.
enum TaskBoardState {
  triage('triage'),
  backlog('backlog'),
  inProgress('in_progress'),
  review('review'),
  done('done'),
  closed('closed');

  const TaskBoardState(this.wireValue);

  /// Stable value carried by the `buzz-board-state` tag.
  final String wireValue;

  /// Interoperable NIP-34 status kind for this state.
  int get statusKind => switch (this) {
    TaskBoardState.triage => 1633,
    TaskBoardState.backlog ||
    TaskBoardState.inProgress ||
    TaskBoardState.review => 1630,
    TaskBoardState.done => 1631,
    TaskBoardState.closed => 1632,
  };

  /// Parse a stable wire value.
  static TaskBoardState parse(String value) => values.firstWhere(
    (state) => state.wireValue == value,
    orElse: () =>
        throw FormatException('invalid board state: unknown value "$value"'),
  );
}

/// Requested automation ceiling. Agent policy may narrow this further.
enum TaskAutomation {
  supervised('supervised'),
  autoCommit('auto_commit'),
  proposePr('propose_pr');

  const TaskAutomation(this.wireValue);

  /// Stable value carried by the `buzz-automation` tag.
  final String wireValue;

  /// Parse a stable wire value, defaulting an absent early-writer value safely.
  static TaskAutomation parse(String? value) {
    if (value == null) return TaskAutomation.supervised;
    return values.firstWhere(
      (automation) => automation.wireValue == value,
      orElse: () =>
          throw FormatException('invalid automation: unknown value "$value"'),
    );
  }
}

/// Public task lifecycle phases emitted by an executor.
enum TaskJobPhase {
  accepted('accepted'),
  waitingForCapacity('waiting_for_capacity'),
  waitingForModel('waiting_for_model'),
  running('running'),
  reviewReady('review_ready'),
  cancelled('cancelled'),
  failed('failed'),
  completed('completed');

  const TaskJobPhase(this.wireValue);

  final String wireValue;

  static TaskJobPhase parse(String value) => values.firstWhere(
    (phase) => phase.wireValue == value,
    orElse: () =>
        throw FormatException('invalid job phase: unknown value "$value"'),
  );
}

/// Complete custom snapshot carried by one NIP-34 status event.
class TaskBoardSnapshot {
  const TaskBoardSnapshot({
    required this.state,
    required this.dependencies,
    required this.baseRef,
    required this.executor,
    required this.automation,
  });

  final TaskBoardState state;
  final List<String> dependencies;
  final String? baseRef;
  final String? executor;
  final TaskAutomation automation;

  /// Parse tags, returning null for a legacy status with no board metadata.
  factory TaskBoardSnapshot.fromTags(List<List<String>> tags) {
    final stateValue = _singletonTag(tags, taskBoardStateTag);
    final hasOtherSnapshotTag = tags.any(
      (tag) =>
          tag.isNotEmpty &&
          const {
            taskDependsOnTag,
            taskBaseRefTag,
            taskExecutorTag,
            taskAutomationTag,
          }.contains(tag.first),
    );
    if (stateValue == null) {
      if (hasOtherSnapshotTag) {
        throw const FormatException(
          'partial task-board snapshot is missing buzz-board-state',
        );
      }
      throw const _LegacySnapshot();
    }

    final dependencies =
        tags
            .where((tag) => tag.isNotEmpty && tag.first == taskDependsOnTag)
            .map(
              (tag) =>
                  _normalizeHex64(tag.length > 1 ? tag[1] : '', 'dependency'),
            )
            .toSet()
            .toList()
          ..sort();
    if (dependencies.length > 100) {
      throw const FormatException('invalid dependencies: maximum is 100');
    }
    final executorValue = _singletonTag(tags, taskExecutorTag);
    return TaskBoardSnapshot(
      state: TaskBoardState.parse(stateValue),
      dependencies: dependencies,
      baseRef: switch (_singletonTag(tags, taskBaseRefTag)) {
        final value? => validateTaskBaseRef(value),
        null => null,
      },
      executor: executorValue == null
          ? null
          : _normalizeHex64(executorValue, 'executor'),
      automation: TaskAutomation.parse(_singletonTag(tags, taskAutomationTag)),
    );
  }

  /// Build canonical custom tags for this complete snapshot.
  List<List<String>> toTags() {
    final canonical = TaskBoardSnapshot.fromTags([
      [taskBoardStateTag, state.wireValue],
      for (final dependency in dependencies) [taskDependsOnTag, dependency],
      if (baseRef != null) [taskBaseRefTag, baseRef!],
      if (executor != null) [taskExecutorTag, executor!],
      [taskAutomationTag, automation.wireValue],
    ]);
    return [
      [taskBoardStateTag, canonical.state.wireValue],
      for (final dependency in canonical.dependencies)
        [taskDependsOnTag, dependency],
      if (canonical.baseRef != null) [taskBaseRefTag, canonical.baseRef!],
      if (canonical.executor != null) [taskExecutorTag, canonical.executor!],
      [taskAutomationTag, canonical.automation.wireValue],
    ];
  }

  Map<String, Object?> toJson() => {
    'state': state.wireValue,
    'dependencies': dependencies,
    'baseRef': baseRef,
    'executor': executor,
    'automation': automation.wireValue,
  };
}

/// Minimal signed-event envelope required by task-board projection.
class TaskBoardEvent {
  const TaskBoardEvent({
    required this.id,
    required this.pubkey,
    required this.kind,
    required this.createdAt,
    required this.tags,
    this.content = '',
  });

  factory TaskBoardEvent.fromJson(Map<String, Object?> json) => TaskBoardEvent(
    id: json['id']! as String,
    pubkey: json['pubkey']! as String,
    kind: json['kind']! as int,
    createdAt: json['created_at']! as int,
    tags: (json['tags']! as List<Object?>)
        .map((tag) => (tag! as List<Object?>).cast<String>())
        .toList(),
    content: json['content'] as String? ?? '',
  );

  final String id;
  final String pubkey;
  final int kind;
  final int createdAt;
  final List<List<String>> tags;
  final String content;

  Iterable<String> tagValues(String name) => tags
      .where((tag) => tag.length > 1 && tag.first == name && tag[1].isNotEmpty)
      .map((tag) => tag[1]);
}

/// Versioned provider-neutral request body for a supervised coding task.
class CodeTaskRequestV1 {
  const CodeTaskRequestV1({
    required this.prompt,
    required this.baseRef,
    required this.automation,
  });

  final String prompt;
  final String baseRef;
  final TaskAutomation automation;

  Map<String, Object> toJson() => {
    'v': 1,
    'operation': 'code-task',
    'prompt': prompt,
    'baseRef': baseRef,
    'automation': automation.wireValue,
  };
}

/// Parsed lifecycle update content for a task request.
class TaskJobUpdateV1 {
  const TaskJobUpdateV1({required this.phase, required this.summary});

  final TaskJobPhase phase;
  final String summary;

  factory TaskJobUpdateV1.fromJson(String content) {
    final decoded = jsonDecode(content);
    if (decoded is! Map<String, dynamic> ||
        decoded.keys.any(
          (key) => !const {'v', 'phase', 'summary'}.contains(key),
        ) ||
        decoded['v'] != 1 ||
        decoded['phase'] is! String ||
        decoded['summary'] is! String) {
      throw const FormatException('invalid task-job update: malformed payload');
    }
    final summary = _validateText(
      decoded['summary'] as String,
      'summary',
      4096,
    );
    return TaskJobUpdateV1(
      phase: TaskJobPhase.parse(decoded['phase'] as String),
      summary: summary,
    );
  }
}

/// Latest valid executor-authored lifecycle state for one task request.
class ProjectedTaskJob {
  const ProjectedTaskJob({
    required this.requestId,
    required this.issueId,
    required this.executor,
    required this.phase,
    required this.summary,
    required this.updatedAt,
    required this.terminalEventId,
  });

  final String requestId;
  final String issueId;
  final String executor;
  final TaskJobPhase phase;
  final String summary;
  final int updatedAt;
  final String? terminalEventId;
}

/// Deterministic projection of one issue and its authorized status history.
class ProjectedTaskBoardIssue {
  const ProjectedTaskBoardIssue({
    required this.issueId,
    required this.snapshot,
    required this.statusEventId,
    required this.updatedAt,
    required this.validationErrors,
  });

  final String issueId;
  final TaskBoardSnapshot snapshot;
  final String? statusEventId;
  final int updatedAt;
  final List<String> validationErrors;

  bool get isExecutable => validationErrors.isEmpty;

  Map<String, Object?> toJson() => {
    'issueId': issueId,
    'snapshot': snapshot.toJson(),
    'statusEventId': statusEventId,
    'updatedAt': updatedAt,
    'validationErrors': validationErrors,
  };
}

/// Missing and cyclic dependency facts for a projected board.
class TaskDependencyAnalysis {
  const TaskDependencyAnalysis({
    required this.missing,
    required this.cycleMembers,
  });

  final Map<String, List<String>> missing;
  final Set<String> cycleMembers;

  bool get isValid => missing.isEmpty && cycleMembers.isEmpty;
}

/// Project one issue using the same authorization and LWW rules as Rust.
ProjectedTaskBoardIssue projectTaskBoardIssue(
  TaskBoardEvent issue,
  List<TaskBoardEvent> statuses, {
  List<String> maintainers = const [],
}) {
  final issueId = _normalizeHex64(issue.id, 'issue id');
  final authorized = {
    _normalizeHex64(issue.pubkey, 'issue author'),
    ...maintainers.map((pubkey) => _normalizeHex64(pubkey, 'maintainer')),
  };
  final owner = _repoOwner(issue);
  if (owner != null) authorized.add(owner);

  final candidates =
      statuses
          .where(
            (event) =>
                _statusKinds.contains(event.kind) &&
                _hex64.hasMatch(event.pubkey) &&
                authorized.contains(event.pubkey.toLowerCase()) &&
                event
                    .tagValues('e')
                    .any((referenced) => referenced.toLowerCase() == issueId),
          )
          .toList()
        ..sort((left, right) {
          final timestampOrder = right.createdAt.compareTo(left.createdAt);
          if (timestampOrder != 0) return timestampOrder;
          return right.id.toLowerCase().compareTo(left.id.toLowerCase());
        });
  final latest = candidates.firstOrNull;
  final fallbackState = _legacyState(
    latest?.kind ?? 1630,
    issue.tagValues('t').toList(),
  );
  var snapshot = TaskBoardSnapshot(
    state: fallbackState,
    dependencies: const [],
    baseRef: null,
    executor: null,
    automation: TaskAutomation.supervised,
  );
  final errors = <String>[];
  if (latest != null) {
    try {
      snapshot = TaskBoardSnapshot.fromTags(latest.tags);
      if (snapshot.state.statusKind != latest.kind) {
        errors.add(
          'board state ${snapshot.state.wireValue} conflicts with NIP-34 status kind ${latest.kind}',
        );
      }
    } on _LegacySnapshot {
      // Standard NIP-34 status with no Buzz snapshot uses the fallback above.
    } on FormatException catch (error) {
      errors.add(error.message.toString());
    }
  }

  return ProjectedTaskBoardIssue(
    issueId: issueId,
    snapshot: snapshot,
    statusEventId: latest?.id.toLowerCase(),
    updatedAt: latest?.createdAt ?? issue.createdAt,
    validationErrors: errors,
  );
}

/// Analyze missing dependencies and all tasks participating in cycles.
TaskDependencyAnalysis analyzeTaskDependencies(
  List<ProjectedTaskBoardIssue> issues,
) {
  final graph = {
    for (final issue in issues) issue.issueId: issue.snapshot.dependencies,
  };
  final missing = <String, List<String>>{};
  for (final entry in graph.entries) {
    final absent = entry.value
        .where((dependency) => !graph.containsKey(dependency))
        .toList();
    if (absent.isNotEmpty) missing[entry.key] = absent;
  }

  final visited = <String>{};
  final active = <String>[];
  final cycleMembers = <String>{};
  void visit(String issueId) {
    final cycleStart = active.indexOf(issueId);
    if (cycleStart >= 0) {
      cycleMembers.addAll(active.sublist(cycleStart));
      return;
    }
    if (!visited.add(issueId)) return;
    active.add(issueId);
    for (final dependency in graph[issueId] ?? const <String>[]) {
      if (graph.containsKey(dependency)) visit(dependency);
    }
    active.removeLast();
  }

  final issueIds = graph.keys.toList()..sort();
  for (final issueId in issueIds) {
    visit(issueId);
  }
  return TaskDependencyAnalysis(missing: missing, cycleMembers: cycleMembers);
}

/// Validate and return a Git ref safe to carry in a signed task event.
String validateTaskBaseRef(String value) {
  _validateText(value, 'base ref', 256);
  const forbidden = {'~', '^', ':', '?', '*', '[', r'\'};
  if (value != value.trim() ||
      RegExp(r'\s').hasMatch(value) ||
      value.contains('..') ||
      value.startsWith('-') ||
      value.endsWith('.') ||
      value.endsWith('/') ||
      value.contains('//') ||
      value.contains('@{') ||
      forbidden.any(value.contains)) {
    throw const FormatException('invalid base ref: is not a safe Git ref name');
  }
  return value;
}

/// Build and validate a provider-neutral code-task request.
CodeTaskRequestV1 buildCodeTaskRequest({
  required String prompt,
  required String baseRef,
  TaskAutomation automation = TaskAutomation.supervised,
}) => CodeTaskRequestV1(
  prompt: _validateText(prompt, 'prompt', 64 * 1024),
  baseRef: validateTaskBaseRef(baseRef),
  automation: automation,
);

/// Build canonical task-request references without provider configuration.
List<List<String>> buildCodeTaskRequestTags({
  required String issueId,
  required String repoAddress,
  required String executor,
  String? projectAddress,
  String? channelId,
}) => [
  ['e', _normalizeHex64(issueId, 'issue id'), '', 'issue'],
  ['a', _validateAddress(repoAddress, 30617, 'repo address')],
  ['p', _normalizeHex64(executor, 'executor pubkey')],
  ['t', 'code-task'],
  if (projectAddress != null)
    ['a', _validateAddress(projectAddress, 30621, 'project address')],
  if (channelId != null) ['h', channelId],
];

/// Build canonical references for progress, result, error, or cancellation.
List<List<String>> buildTaskJobLifecycleTags({
  required String requestId,
  required String issueId,
  String? channelId,
}) => [
  ['e', _normalizeHex64(requestId, 'request id'), '', 'root'],
  ['e', _normalizeHex64(issueId, 'issue id'), '', 'issue'],
  if (channelId != null) ['h', channelId],
];

/// Project the latest valid executor-authored lifecycle update.
ProjectedTaskJob projectTaskJob(
  TaskBoardEvent request,
  List<TaskBoardEvent> lifecycle,
) {
  if (request.kind != 43001) {
    throw const FormatException('invalid job request kind');
  }
  _parseCodeTaskRequest(request.content);
  final requestId = _normalizeHex64(request.id, 'request id');
  final issueId = request.tags
      .where((tag) => tag.length > 3 && tag[0] == 'e' && tag[3] == 'issue')
      .map((tag) => tag[1])
      .firstOrNull;
  final executor = request.tagValues('p').firstOrNull;
  if (issueId == null || executor == null) {
    throw const FormatException('invalid job request references');
  }
  final normalizedIssue = _normalizeHex64(issueId, 'issue id');
  final normalizedExecutor = _normalizeHex64(executor, 'executor pubkey');
  final candidates =
      lifecycle.where((event) {
        return _jobUpdateKinds.contains(event.kind) &&
            event.pubkey.toLowerCase() == normalizedExecutor &&
            event.tags.any(
              (tag) =>
                  tag.length > 3 &&
                  tag[0] == 'e' &&
                  tag[1].toLowerCase() == requestId &&
                  tag[3] == 'root',
            ) &&
            event.tags.any(
              (tag) =>
                  tag.length > 3 &&
                  tag[0] == 'e' &&
                  tag[1].toLowerCase() == normalizedIssue &&
                  tag[3] == 'issue',
            );
      }).toList()..sort((left, right) {
        final timeOrder = right.createdAt.compareTo(left.createdAt);
        if (timeOrder != 0) return timeOrder;
        return right.id.toLowerCase().compareTo(left.id.toLowerCase());
      });
  TaskBoardEvent? latest;
  var phase = TaskJobPhase.accepted;
  var summary = 'Task request published.';
  for (final candidate in candidates) {
    try {
      final update = TaskJobUpdateV1.fromJson(candidate.content);
      latest = candidate;
      phase = update.phase;
      summary = update.summary;
      break;
    } on FormatException {
      // A malformed event cannot hide the last valid lifecycle update.
    }
  }
  return ProjectedTaskJob(
    requestId: requestId,
    issueId: normalizedIssue,
    executor: normalizedExecutor,
    phase: phase,
    summary: summary,
    updatedAt: latest?.createdAt ?? request.createdAt,
    terminalEventId: latest?.id.toLowerCase(),
  );
}

Map<String, Object?> _parseCodeTaskRequest(String content) {
  final decoded = jsonDecode(content);
  const allowed = {'v', 'operation', 'prompt', 'baseRef', 'automation'};
  if (decoded is! Map<String, dynamic> ||
      decoded.keys.any((key) => !allowed.contains(key)) ||
      decoded['v'] != 1 ||
      decoded['operation'] != 'code-task' ||
      decoded['prompt'] is! String ||
      decoded['baseRef'] is! String ||
      decoded['automation'] is! String) {
    throw const FormatException('invalid code-task request: malformed payload');
  }
  _validateText(decoded['prompt'] as String, 'prompt', 64 * 1024);
  validateTaskBaseRef(decoded['baseRef'] as String);
  TaskAutomation.parse(decoded['automation'] as String);
  return decoded;
}

String _validateAddress(String value, int kind, String field) {
  final match = RegExp('^$kind:([0-9a-fA-F]{64}):(.+)\$').firstMatch(value);
  if (match == null) {
    throw FormatException('invalid $field: must use kind $kind');
  }
  return '$kind:${match.group(1)!.toLowerCase()}:${match.group(2)}';
}

String _validateText(String value, String field, int maximumBytes) {
  if (value.trim().isEmpty) {
    throw FormatException('invalid $field: must not be empty');
  }
  if (utf8.encode(value).length > maximumBytes) {
    throw FormatException(
      'invalid $field: must not exceed $maximumBytes bytes',
    );
  }
  for (final rune in value.runes) {
    if ((rune < 32 || rune == 127) && !const {9, 10, 13}.contains(rune)) {
      throw FormatException(
        'invalid $field: contains unsupported control characters',
      );
    }
  }
  return value;
}

String? _singletonTag(List<List<String>> tags, String name) {
  final matching = tags.where((tag) => tag.isNotEmpty && tag.first == name);
  if (matching.length > 1) {
    throw FormatException('duplicate singleton tag: $name');
  }
  if (matching.isEmpty) return null;
  final tag = matching.single;
  if (tag.length < 2 || tag[1].isEmpty) {
    throw FormatException('invalid $name: missing tag value');
  }
  return tag[1];
}

String _normalizeHex64(String value, String field) {
  if (!_hex64.hasMatch(value)) {
    throw FormatException(
      'invalid $field: must be exactly 64 hexadecimal characters',
    );
  }
  return value.toLowerCase();
}

String? _repoOwner(TaskBoardEvent issue) {
  for (final address in issue.tagValues('a')) {
    final match = RegExp(r'^30617:([0-9a-fA-F]{64}):').firstMatch(address);
    if (match != null) return match.group(1)!.toLowerCase();
  }
  return null;
}

TaskBoardState _legacyState(int kind, List<String> labels) {
  if (kind == 1631) return TaskBoardState.done;
  if (kind == 1632) return TaskBoardState.closed;
  if (kind == 1633) return TaskBoardState.triage;
  final normalized = labels.map((label) => label.toLowerCase()).toSet();
  if (normalized.contains('in-review') || normalized.contains('review')) {
    return TaskBoardState.review;
  }
  if (normalized.contains('in-progress') || normalized.contains('active')) {
    return TaskBoardState.inProgress;
  }
  if (normalized.contains('triage')) return TaskBoardState.triage;
  return TaskBoardState.backlog;
}

class _LegacySnapshot implements Exception {
  const _LegacySnapshot();
}
