import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../shared/task_board/task_board_protocol.dart';
import '../../shared/theme/theme.dart';
import 'task_board_provider.dart';

const _columns = [
  (TaskBoardState.triage, 'Triage'),
  (TaskBoardState.backlog, 'Backlog'),
  (TaskBoardState.inProgress, 'In progress'),
  (TaskBoardState.review, 'Review'),
  (TaskBoardState.done, 'Done'),
  (TaskBoardState.closed, 'Closed'),
];

class TaskBoardPage extends HookConsumerWidget {
  const TaskBoardPage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final repositories = ref.watch(taskBoardRepositoriesProvider);
    final selected = useState<TaskBoardRepository?>(null);
    final available = repositories.value ?? const <TaskBoardRepository>[];
    final repository = available.contains(selected.value)
        ? selected.value
        : available.firstOrNull;
    final board = repository == null
        ? null
        : ref.watch(taskBoardItemsProvider(repository));

    return Scaffold(
      appBar: AppBar(
        title: const Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(LucideIcons.columns3, size: 20),
            SizedBox(width: Grid.sm),
            Text('Task board'),
          ],
        ),
        actions: [
          IconButton(
            onPressed: repository == null
                ? null
                : () => ref.invalidate(taskBoardItemsProvider(repository)),
            icon: const Icon(LucideIcons.refreshCw),
            tooltip: 'Refresh board',
          ),
        ],
      ),
      body: repositories.when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (error, _) =>
            _BoardMessage(message: 'Could not load repositories: $error'),
        data: (_) {
          if (available.isEmpty) {
            return const _BoardMessage(
              message:
                  'No NIP-34 repositories are available in this community.',
            );
          }
          return Column(
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(
                  Grid.gutter,
                  Grid.sm,
                  Grid.gutter,
                  Grid.sm,
                ),
                child: DropdownButtonFormField<TaskBoardRepository>(
                  decoration: const InputDecoration(labelText: 'Repository'),
                  initialValue: repository,
                  items: [
                    for (final candidate in available)
                      DropdownMenuItem(
                        value: candidate,
                        child: Text(candidate.name),
                      ),
                  ],
                  onChanged: (value) => selected.value = value,
                ),
              ),
              Expanded(
                child:
                    board?.when(
                      loading: () =>
                          const Center(child: CircularProgressIndicator()),
                      error: (error, _) => _BoardMessage(
                        message: 'Could not load the task board: $error',
                      ),
                      data: (items) =>
                          _BoardColumns(repository: repository!, items: items),
                    ) ??
                    const SizedBox.shrink(),
              ),
            ],
          );
        },
      ),
    );
  }
}

class _BoardColumns extends ConsumerWidget {
  const _BoardColumns({required this.repository, required this.items});

  final TaskBoardRepository repository;
  final List<MobileTaskBoardItem> items;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (items.isEmpty) {
      return const _BoardMessage(
        message: 'No NIP-34 tasks have been published for this repository.',
      );
    }
    final analysis = analyzeTaskDependencies(
      items.map((item) => item.projection).toList(),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (!analysis.isValid)
          Container(
            margin: const EdgeInsets.fromLTRB(
              Grid.gutter,
              0,
              Grid.gutter,
              Grid.sm,
            ),
            padding: const EdgeInsets.all(Grid.sm),
            decoration: BoxDecoration(
              color: context.colors.errorContainer,
              borderRadius: BorderRadius.circular(Radii.md),
            ),
            child: Text(
              'Dependency errors block execution until corrected.',
              style: TextStyle(color: context.colors.onErrorContainer),
            ),
          ),
        Expanded(
          child: ListView.separated(
            padding: const EdgeInsets.fromLTRB(
              Grid.gutter,
              0,
              Grid.gutter,
              Grid.gutter,
            ),
            scrollDirection: Axis.horizontal,
            itemCount: _columns.length,
            separatorBuilder: (_, _) => const SizedBox(width: Grid.sm),
            itemBuilder: (context, index) {
              final column = _columns[index];
              final columnItems = items
                  .where((item) => item.projection.snapshot.state == column.$1)
                  .toList();
              return SizedBox(
                width: 280,
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    color: context.colors.surfaceContainerLow,
                    borderRadius: BorderRadius.circular(Radii.lg),
                  ),
                  child: Column(
                    children: [
                      Padding(
                        padding: const EdgeInsets.all(Grid.sm),
                        child: Row(
                          children: [
                            Expanded(
                              child: Text(
                                column.$2.toUpperCase(),
                                style: context.textTheme.labelSmall,
                              ),
                            ),
                            Text('${columnItems.length}'),
                          ],
                        ),
                      ),
                      Expanded(
                        child: ListView.separated(
                          padding: const EdgeInsets.all(Grid.sm),
                          itemCount: columnItems.length,
                          separatorBuilder: (_, _) =>
                              const SizedBox(height: Grid.sm),
                          itemBuilder: (context, itemIndex) => _TaskCard(
                            repository: repository,
                            item: columnItems[itemIndex],
                            allItems: items,
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              );
            },
          ),
        ),
      ],
    );
  }
}

class _TaskCard extends ConsumerWidget {
  const _TaskCard({
    required this.repository,
    required this.item,
    required this.allItems,
  });

  final TaskBoardRepository repository;
  final MobileTaskBoardItem item;
  final List<MobileTaskBoardItem> allItems;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final snapshot = item.projection.snapshot;
    return Card(
      margin: EdgeInsets.zero,
      child: InkWell(
        borderRadius: BorderRadius.circular(Radii.md),
        onTap: () => showModalBottomSheet<void>(
          context: context,
          isScrollControlled: true,
          builder: (_) => _TaskEditorSheet(
            repository: repository,
            item: item,
            allItems: allItems,
          ),
        ),
        child: Padding(
          padding: const EdgeInsets.all(Grid.sm),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    child: Text(
                      item.title,
                      style: context.textTheme.titleSmall,
                    ),
                  ),
                  PopupMenuButton<TaskBoardState>(
                    icon: const Icon(LucideIcons.ellipsis, size: 18),
                    onSelected: (state) => _run(
                      context,
                      () => ref
                          .read(taskBoardActionsProvider)
                          .update(
                            repository: repository,
                            item: item,
                            snapshot: TaskBoardSnapshot(
                              state: state,
                              dependencies: snapshot.dependencies,
                              baseRef: snapshot.baseRef,
                              executor: snapshot.executor,
                              automation: snapshot.automation,
                            ),
                          ),
                    ),
                    itemBuilder: (_) => [
                      for (final column in _columns)
                        PopupMenuItem(value: column.$1, child: Text(column.$2)),
                    ],
                  ),
                ],
              ),
              Text(
                '#${item.event.id.substring(0, 8)}',
                style: context.textTheme.bodySmall?.copyWith(
                  color: context.colors.onSurfaceVariant,
                ),
              ),
              if (snapshot.dependencies.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: Grid.half),
                  child: Text(
                    '${snapshot.dependencies.length} dependencies',
                    style: context.textTheme.bodySmall,
                  ),
                ),
              if (item.job case final job?)
                Container(
                  margin: const EdgeInsets.only(top: Grid.sm),
                  padding: const EdgeInsets.all(Grid.sm),
                  decoration: BoxDecoration(
                    color: context.colors.primaryContainer,
                    borderRadius: BorderRadius.circular(Radii.sm),
                  ),
                  child: Text(
                    '${job.phase.wireValue.replaceAll('_', ' ')} · ${job.summary}',
                    style: context.textTheme.bodySmall,
                  ),
                ),
              if (item.projection.validationErrors.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: Grid.sm),
                  child: Text(
                    item.projection.validationErrors.first,
                    style: TextStyle(color: context.colors.error),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class _TaskEditorSheet extends HookConsumerWidget {
  const _TaskEditorSheet({
    required this.repository,
    required this.item,
    required this.allItems,
  });

  final TaskBoardRepository repository;
  final MobileTaskBoardItem item;
  final List<MobileTaskBoardItem> allItems;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final current = item.projection.snapshot;
    final executor = useTextEditingController(text: current.executor ?? '');
    final baseRef = useTextEditingController(text: current.baseRef ?? 'main');
    final dependencies = useState(current.dependencies.toSet());
    final saving = useState(false);

    TaskBoardSnapshot snapshot() => TaskBoardSnapshot(
      state: current.state,
      dependencies: dependencies.value.toList()..sort(),
      baseRef: baseRef.text.trim().isEmpty ? null : baseRef.text.trim(),
      executor: executor.text.trim().isEmpty
          ? null
          : executor.text.trim().toLowerCase(),
      automation: TaskAutomation.supervised,
    );

    Future<void> perform(bool start) async {
      final next = snapshot();
      final projected = [
        for (final candidate in allItems)
          candidate.event.id == item.event.id
              ? ProjectedTaskBoardIssue(
                  issueId: candidate.event.id,
                  snapshot: next,
                  statusEventId: candidate.projection.statusEventId,
                  updatedAt: candidate.projection.updatedAt,
                  validationErrors: candidate.projection.validationErrors,
                )
              : candidate.projection,
      ];
      if (!analyzeTaskDependencies(projected).isValid) {
        _showError(context, 'Resolve missing or cyclic dependencies first.');
        return;
      }
      saving.value = true;
      try {
        final actions = ref.read(taskBoardActionsProvider);
        if (start) {
          await actions.start(
            repository: repository,
            item: item,
            snapshot: next,
          );
        } else {
          await actions.update(
            repository: repository,
            item: item,
            snapshot: next,
          );
        }
        if (context.mounted) Navigator.of(context).pop();
      } catch (error) {
        if (context.mounted) _showError(context, error.toString());
      } finally {
        saving.value = false;
      }
    }

    return SafeArea(
      child: Padding(
        padding: EdgeInsets.fromLTRB(
          Grid.gutter,
          Grid.gutter,
          Grid.gutter,
          MediaQuery.viewInsetsOf(context).bottom + Grid.gutter,
        ),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(item.title, style: context.textTheme.titleLarge),
              const SizedBox(height: Grid.gutter),
              TextField(
                controller: executor,
                decoration: const InputDecoration(
                  labelText: 'Executor pubkey',
                  hintText: '64-character agent pubkey',
                ),
              ),
              const SizedBox(height: Grid.sm),
              TextField(
                controller: baseRef,
                decoration: const InputDecoration(labelText: 'Base ref'),
              ),
              const SizedBox(height: Grid.gutter),
              Text('Depends on', style: context.textTheme.titleSmall),
              for (final candidate in allItems)
                if (candidate.event.id != item.event.id)
                  CheckboxListTile(
                    contentPadding: EdgeInsets.zero,
                    title: Text(candidate.title),
                    value: dependencies.value.contains(candidate.event.id),
                    onChanged: saving.value
                        ? null
                        : (checked) {
                            final next = {...dependencies.value};
                            if (checked ?? false) {
                              next.add(candidate.event.id);
                            } else {
                              next.remove(candidate.event.id);
                            }
                            dependencies.value = next;
                          },
                  ),
              const SizedBox(height: Grid.sm),
              OutlinedButton(
                onPressed: saving.value ? null : () => perform(false),
                child: const Text('Save settings'),
              ),
              const SizedBox(height: Grid.sm),
              FilledButton.icon(
                onPressed: saving.value || repository.channelId == null
                    ? null
                    : () => perform(true),
                icon: const Icon(LucideIcons.play),
                label: const Text('Start supervised run'),
              ),
              if (repository.channelId == null)
                const Padding(
                  padding: EdgeInsets.only(top: Grid.sm),
                  child: Text(
                    'Bind this repository to a channel before starting an executor.',
                    textAlign: TextAlign.center,
                  ),
                ),
              if (item.job case final job?)
                if (job.phase != TaskJobPhase.cancelled &&
                    job.phase != TaskJobPhase.failed &&
                    job.phase != TaskJobPhase.completed)
                  Padding(
                    padding: const EdgeInsets.only(top: Grid.sm),
                    child: TextButton(
                      onPressed: saving.value
                          ? null
                          : () async {
                              saving.value = true;
                              try {
                                await ref
                                    .read(taskBoardActionsProvider)
                                    .cancel(repository: repository, item: item);
                                if (context.mounted) {
                                  Navigator.of(context).pop();
                                }
                              } catch (error) {
                                if (context.mounted) {
                                  _showError(context, error.toString());
                                }
                              } finally {
                                saving.value = false;
                              }
                            },
                      child: const Text('Cancel task'),
                    ),
                  ),
            ],
          ),
        ),
      ),
    );
  }
}

class _BoardMessage extends StatelessWidget {
  const _BoardMessage({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) => Center(
    child: Padding(
      padding: const EdgeInsets.all(Grid.gutter),
      child: Text(message, textAlign: TextAlign.center),
    ),
  );
}

Future<void> _run(
  BuildContext context,
  Future<void> Function() operation,
) async {
  try {
    await operation();
  } catch (error) {
    if (context.mounted) _showError(context, error.toString());
  }
}

void _showError(BuildContext context, String message) {
  ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(message)));
}
