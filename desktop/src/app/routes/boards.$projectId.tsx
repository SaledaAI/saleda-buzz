import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const TaskBoardScreen = React.lazy(async () => {
  const module = await import("@/features/task-board/ui/TaskBoardScreen");
  return { default: module.TaskBoardScreen };
});

export const Route = createFileRoute("/boards/$projectId")({
  component: TaskBoardRouteComponent,
});

function TaskBoardRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <TaskBoardScreen projectId={projectId} />
    </React.Suspense>
  );
}
