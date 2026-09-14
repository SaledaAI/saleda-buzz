import 'dart:async';
import 'dart:convert';

import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../shared/relay/relay.dart';
import '../../shared/task_board/task_board_protocol.dart';

const _boardKinds = [1621, 1630, 1631, 1632, 1633];
const _jobKinds = [43001, 43002, 43003, 43004, 43005, 43006];

class TaskBoardRepository {
  const TaskBoardRepository({
    required this.id,
    required this.name,
    required this.owner,
    required this.channelId,
  });

  final String id;
  final String name;
  final String owner;
  final String? channelId;

  String get address => '30617:$owner:$id';

  @override
  bool operator ==(Object other) =>
      other is TaskBoardRepository && other.address == address;

  @override
  int get hashCode => address.hashCode;
}

class MobileTaskBoardItem {
  const MobileTaskBoardItem({
    required this.event,
    required this.projection,
    required this.title,
    required this.job,
  });

  final TaskBoardEvent event;
  final ProjectedTaskBoardIssue projection;
  final String title;
  final ProjectedTaskJob? job;
}

final taskBoardRepositoriesProvider = FutureProvider<List<TaskBoardRepository>>(
  (ref) async {
    final events = await ref.watch(relaySessionProvider.notifier).queryRelay([
      const NostrFilter(kinds: [30617], limit: 500),
    ]);
    final latest = <String, NostrEvent>{};
    for (final event in events) {
      final id = event.getTagValue('d') ?? event.id;
      final key = '${event.pubkey}:$id';
      final previous = latest[key];
      if (previous == null ||
          event.createdAt > previous.createdAt ||
          (event.createdAt == previous.createdAt &&
              event.id.compareTo(previous.id) > 0)) {
        latest[key] = event;
      }
    }
    return [
      for (final event in latest.values)
        TaskBoardRepository(
          id: event.getTagValue('d') ?? event.id,
          name:
              event.getTagValue('name') ??
              event.getTagValue('d') ??
              'Repository',
          owner: event.pubkey.toLowerCase(),
          channelId: event.getTagValue('buzz-channel'),
        ),
    ]..sort((left, right) => left.name.compareTo(right.name));
  },
);

final taskBoardItemsProvider =
    FutureProvider.family<List<MobileTaskBoardItem>, TaskBoardRepository>((
      ref,
      repository,
    ) async {
      final session = ref.watch(relaySessionProvider.notifier);
      final boardEvents = await session.queryRelay([
        NostrFilter(
          kinds: _boardKinds,
          tags: {
            '#a': [repository.address],
          },
          limit: 2000,
        ),
      ]);
      final issues = boardEvents.where((event) => event.kind == 1621).toList();
      final issueIds = issues.map((event) => event.id).toList();
      final jobs = issueIds.isEmpty
          ? const <NostrEvent>[]
          : await session.queryRelay([
              NostrFilter(
                kinds: _jobKinds,
                tags: {'#e': issueIds},
                limit: 2000,
              ),
            ]);
      final taskEvents = boardEvents.map(_taskEvent).toList();
      final taskJobs = jobs.map(_taskEvent).toList();
      final statuses = taskEvents.where((event) => event.kind != 1621).toList();
      final items = <MobileTaskBoardItem>[];
      for (final issue in issues) {
        final event = _taskEvent(issue);
        final projection = projectTaskBoardIssue(event, statuses);
        final allowed = {event.pubkey.toLowerCase(), repository.owner};
        final requests =
            taskJobs.where((candidate) {
              return candidate.kind == 43001 &&
                  allowed.contains(candidate.pubkey.toLowerCase()) &&
                  candidate.tags.any(
                    (tag) =>
                        tag.length > 3 &&
                        tag[0] == 'e' &&
                        tag[1].toLowerCase() == event.id.toLowerCase() &&
                        tag[3] == 'issue',
                  );
            }).toList()..sort((left, right) {
              final timeOrder = right.createdAt.compareTo(left.createdAt);
              return timeOrder != 0
                  ? timeOrder
                  : right.id.toLowerCase().compareTo(left.id.toLowerCase());
            });
        ProjectedTaskJob? job;
        for (final request in requests) {
          try {
            job = projectTaskJob(request, taskJobs);
            break;
          } on FormatException {
            // Ignore malformed or unsupported requests.
          }
        }
        final subject = issue.getTagValue('subject');
        final firstLine = issue.content.split('\n').first.trim();
        items.add(
          MobileTaskBoardItem(
            event: event,
            projection: projection,
            title: subject?.isNotEmpty == true
                ? subject!
                : firstLine.isNotEmpty
                ? firstLine
                : 'Untitled task',
            job: job,
          ),
        );
      }
      items.sort((left, right) {
        final timeOrder = right.projection.updatedAt.compareTo(
          left.projection.updatedAt,
        );
        return timeOrder != 0 ? timeOrder : left.title.compareTo(right.title);
      });

      var disposed = false;
      final unsubscribers = <void Function()>[];
      ref.onDispose(() {
        disposed = true;
        for (final unsubscribe in unsubscribers) {
          unsubscribe();
        }
      });
      Future<void> subscribe() async {
        final filters = [
          NostrFilter(
            kinds: _boardKinds,
            tags: {
              '#a': [repository.address],
            },
          ),
          if (issueIds.isNotEmpty)
            NostrFilter(kinds: _jobKinds, tags: {'#e': issueIds}),
        ];
        for (final filter in filters) {
          final unsubscribe = await session.subscribe(filter, (_) {
            if (!disposed) ref.invalidateSelf();
          });
          if (disposed) {
            unsubscribe();
          } else {
            unsubscribers.add(unsubscribe);
          }
        }
      }

      unawaited(subscribe());
      return items;
    });

class TaskBoardActions {
  const TaskBoardActions(this._ref);

  final Ref _ref;

  Future<void> update({
    required TaskBoardRepository repository,
    required MobileTaskBoardItem item,
    required TaskBoardSnapshot snapshot,
  }) async {
    _ensureAuthorized(repository, item);
    await _relay.submit(
      kind: snapshot.state.statusKind,
      content: 'Moved to ${snapshot.state.wireValue.replaceAll('_', ' ')}',
      tags: [
        ['e', item.event.id, '', 'root'],
        ['a', repository.address],
        ['p', repository.owner],
        ...snapshot.toTags(),
      ],
    );
    _ref.invalidate(taskBoardItemsProvider(repository));
  }

  Future<void> start({
    required TaskBoardRepository repository,
    required MobileTaskBoardItem item,
    required TaskBoardSnapshot snapshot,
  }) async {
    final channelId = repository.channelId;
    final executor = snapshot.executor;
    if (channelId == null || channelId.isEmpty) {
      throw StateError('Bind this repository to a channel before starting.');
    }
    if (executor == null) throw StateError('Select an executor first.');
    if (snapshot.automation != TaskAutomation.supervised) {
      throw StateError('Task-board v1 starts supervised runs only.');
    }
    await update(
      repository: repository,
      item: item,
      snapshot: TaskBoardSnapshot(
        state: TaskBoardState.inProgress,
        dependencies: snapshot.dependencies,
        baseRef: snapshot.baseRef,
        executor: executor,
        automation: TaskAutomation.supervised,
      ),
    );
    final request = buildCodeTaskRequest(
      prompt: [
        'Implement task: ${item.title}',
        item.event.content.trim(),
        'This is a supervised run. Work in an isolated Git worktree and leave a tested diff for human review. Do not commit, push, open a pull request, merge, resolve the issue, or remove the worktree.',
      ].where((part) => part.isNotEmpty).join('\n\n'),
      baseRef: snapshot.baseRef ?? 'main',
    );
    await _relay.submit(
      kind: EventKind.jobRequest,
      content: jsonEncode(request.toJson()),
      tags: buildCodeTaskRequestTags(
        issueId: item.event.id,
        repoAddress: repository.address,
        executor: executor,
        channelId: channelId,
      ),
    );
    _ref.invalidate(taskBoardItemsProvider(repository));
  }

  Future<void> cancel({
    required TaskBoardRepository repository,
    required MobileTaskBoardItem item,
  }) async {
    _ensureAuthorized(repository, item);
    final job = item.job;
    final channelId = repository.channelId;
    if (job == null || channelId == null) {
      throw StateError('This task does not have an active executor request.');
    }
    await _relay.submit(
      kind: EventKind.jobCancel,
      content: jsonEncode({
        'v': 1,
        'operation': 'cancel',
        'summary': 'Cancellation requested by the task owner.',
      }),
      tags: [
        ...buildTaskJobLifecycleTags(
          requestId: job.requestId,
          issueId: item.event.id,
          channelId: channelId,
        ),
        ['p', job.executor],
      ],
    );
    _ref.invalidate(taskBoardItemsProvider(repository));
  }

  SignedEventRelay get _relay {
    final config = _ref.read(relayConfigProvider);
    return SignedEventRelay(
      session: _ref.read(relaySessionProvider.notifier),
      nsec: config.nsec,
    );
  }

  void _ensureAuthorized(
    TaskBoardRepository repository,
    MobileTaskBoardItem item,
  ) {
    final pubkey = _ref.read(myPubkeyProvider)?.toLowerCase();
    if (pubkey == null ||
        (pubkey != repository.owner &&
            pubkey != item.event.pubkey.toLowerCase())) {
      throw StateError(
        'Only the task author or repository owner can update it.',
      );
    }
  }
}

final taskBoardActionsProvider = Provider<TaskBoardActions>(
  TaskBoardActions.new,
);

TaskBoardEvent _taskEvent(NostrEvent event) => TaskBoardEvent(
  id: event.id,
  pubkey: event.pubkey,
  kind: event.kind,
  createdAt: event.createdAt,
  tags: event.tags,
  content: event.content,
);
