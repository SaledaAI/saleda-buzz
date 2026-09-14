import { createFileRoute } from "@tanstack/react-router";
import { TaskBoardPage } from "@/features/task-board/ui/TaskBoardPage";

export const Route = createFileRoute("/boards/$repoId")({
  component: TaskBoardRoute,
});

function TaskBoardRoute() {
  const { repoId } = Route.useParams();
  return <TaskBoardPage repoId={repoId} />;
}
