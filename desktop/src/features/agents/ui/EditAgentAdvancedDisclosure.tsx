import type { ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { cn } from "@/shared/lib/cn";
import { ADVANCED_FIELDS_MOTION_TRANSITION } from "./agentConfigOptions";
import { AdvancedRequiredBadge } from "./AdvancedRequiredBadge";

/** User-controlled disclosure for the instance editor's advanced fields. */
export function EditAgentAdvancedDisclosure({
  children,
  disabled,
  envVars,
  requiredEnvKeys,
  open,
  onToggle,
}: {
  children: ReactNode;
  disabled: boolean;
  envVars: Record<string, string>;
  requiredEnvKeys: readonly string[];
  open: boolean;
  onToggle: () => void;
}) {
  const reduceMotion = useReducedMotion();
  return (
    <div className="space-y-3">
      <button
        aria-expanded={open}
        className="inline-flex h-9 items-center gap-1.5 text-sm font-medium text-foreground transition-colors hover:text-foreground/80 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
        disabled={disabled}
        onClick={onToggle}
        type="button"
      >
        <span>Advanced</span>
        <AdvancedRequiredBadge
          envVars={envVars}
          requiredEnvKeys={requiredEnvKeys}
          testId="edit-agent-advanced-required-badge"
        />
        <ChevronDown
          className={cn(
            "h-4 w-4 text-muted-foreground transition-transform duration-150 ease-out",
            open && "rotate-180",
          )}
        />
      </button>
      <AnimatePresence initial={false}>
        {open ? (
          <motion.div
            animate={{ height: "auto", opacity: 1, scale: 1 }}
            className="origin-top overflow-hidden"
            exit={{ height: 0, opacity: 0, scale: 0.98 }}
            initial={{ height: 0, opacity: 0, scale: 0.98 }}
            key="edit-agent-advanced-fields"
            transition={
              reduceMotion ? { duration: 0 } : ADVANCED_FIELDS_MOTION_TRANSITION
            }
          >
            {children}
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}
