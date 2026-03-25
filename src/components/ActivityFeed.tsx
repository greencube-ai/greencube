import type { AuditEntry } from '../lib/types';
import { EmptyState } from './EmptyState';

interface ActivityFeedProps {
  entries: AuditEntry[];
  agentNames?: Record<string, string>;
}

const actionColors: Record<string, string> = {
  tool_call: '#3b82f6',
  llm_request: '#a855f7',
  llm_response: '#a855f7',
  permission_check: '#eab308',
  error: '#ef4444',
  task_start: '#22c55e',
  task_end: '#22c55e',
};

export function ActivityFeed({ entries, agentNames }: ActivityFeedProps) {
  if (entries.length === 0) {
    return <EmptyState message="No activity yet" />;
  }

  return (
    <div className="space-y-1 max-h-[600px] overflow-y-auto">
      {entries.map((entry) => {
        const color = actionColors[entry.action_type] || 'var(--text-muted)';
        const agentName = agentNames?.[entry.agent_id];
        const time = new Date(entry.created_at).toLocaleTimeString();

        // Try to extract a summary from action_detail
        let summary = entry.action_type;
        try {
          const detail = JSON.parse(entry.action_detail);
          if (detail.tool) summary = `${detail.tool}`;
          if (detail.command) summary = `${detail.tool}: ${detail.command}`.slice(0, 60);
        } catch {
          // Use action_type as fallback
        }

        return (
          <div
            key={entry.id}
            className="flex items-start gap-2 px-2 py-1.5 rounded text-xs hover:bg-[var(--bg-hover)] activity-slide-in"
          >
            <span className="text-[var(--text-muted)] flex-shrink-0 w-16">
              {time}
            </span>
            {agentName && (
              <span className="text-[var(--text-secondary)] flex-shrink-0">
                {agentName}
              </span>
            )}
            <span
              className="px-1.5 py-0.5 rounded text-[10px] font-medium flex-shrink-0"
              style={{ backgroundColor: color + '20', color }}
            >
              {entry.action_type}
            </span>
            <span className="text-[var(--text-secondary)] truncate">{summary}</span>
            {entry.permission_result === 'denied' && (
              <span className="text-[var(--status-error)] flex-shrink-0">denied</span>
            )}
          </div>
        );
      })}
    </div>
  );
}
