import type { StageStatus, StageView } from "../api/types";
import { duration } from "../format";

const STATUS_LABEL: Record<StageStatus, string> = {
  idle: "waiting",
  running: "running",
  done: "done",
  failed: "failed",
  skipped: "skipped",
};

interface StageChainProps {
  stages: StageView[];
  now: number;
}

function elapsed(stage: StageView, now: number): string | null {
  if (stage.started_at === null) {
    return null;
  }
  const end = stage.finished_at ?? now;
  return duration(end - stage.started_at);
}

export function StageChain({ stages, now }: StageChainProps) {
  return (
    <ol className="chain" aria-label="login stages">
      {stages.map((stage, index) => {
        const took = stage.status === "skipped" ? null : elapsed(stage, now);
        return (
          <li key={stage.id} className={`stage stage--${stage.status}`} title={stage.detail || undefined}>
            <span className="stage-node" aria-hidden="true">
              {index + 1}
            </span>
            <span className="stage-text">
              <span className="stage-label">{stage.label}</span>
              <span className="stage-status">
                {STATUS_LABEL[stage.status]}
                {took === null ? "" : ` · ${took}`}
              </span>
              {stage.detail ? <span className="stage-detail">{stage.detail}</span> : null}
            </span>
          </li>
        );
      })}
    </ol>
  );
}
