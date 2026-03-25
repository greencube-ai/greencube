import type { Episode } from '../lib/types';
import { EmptyState } from './EmptyState';

interface MemoryViewerProps {
  episodes: Episode[];
}

const eventColors: Record<string, string> = {
  tool_call: '#3b82f6',
  llm_response: '#a855f7',
  llm_request: '#a855f7',
  task_start: '#22c55e',
  task_end: '#22c55e',
  error: '#ef4444',
};

export function MemoryViewer({ episodes }: MemoryViewerProps) {
  if (episodes.length === 0) {
    return <EmptyState message="No memories yet. This agent hasn't run any tasks." />;
  }

  return (
    <div className="relative pl-6">
      {/* Timeline line */}
      <div
        className="absolute left-2 top-0 bottom-0 w-0.5"
        style={{ backgroundColor: 'var(--accent)', opacity: 0.3 }}
      />

      <div className="space-y-3">
        {episodes.map((ep) => {
          const color = eventColors[ep.event_type] || 'var(--text-muted)';
          const time = new Date(ep.created_at).toLocaleString();

          return (
            <div key={ep.id} className="relative">
              {/* Timeline dot */}
              <div
                className="absolute -left-[18px] top-2 w-2 h-2 rounded-full"
                style={{ backgroundColor: color }}
              />

              <div
                className="p-3 rounded-lg border"
                style={{
                  backgroundColor: 'var(--bg-secondary)',
                  borderColor: 'var(--border)',
                }}
              >
                <div className="flex items-center gap-2 mb-1">
                  <span className="text-[10px] text-[var(--text-muted)]">{time}</span>
                  <span
                    className="px-1.5 py-0.5 rounded text-[10px] font-medium"
                    style={{ backgroundColor: color + '20', color }}
                  >
                    {ep.event_type}
                  </span>
                  {ep.outcome && (
                    <span
                      className="px-1.5 py-0.5 rounded text-[10px]"
                      style={{
                        color: ep.outcome === 'success' ? 'var(--status-active)' : 'var(--status-error)',
                      }}
                    >
                      {ep.outcome}
                    </span>
                  )}
                </div>
                <p className="text-sm text-[var(--text-primary)]">{ep.summary}</p>
                {ep.task_id && (
                  <span className="text-[10px] text-[var(--text-muted)]">
                    Task: {ep.task_id.slice(0, 8)}...
                  </span>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
