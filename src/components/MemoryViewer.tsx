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
  reflection: '#a855f7',
  error: '#ef4444',
  spawn: '#eab308',
};

export function MemoryViewer({ episodes }: MemoryViewerProps) {
  if (episodes.length === 0) {
    return <EmptyState message="No memories yet" subtitle="Connect this agent via the Connect page. Memories form as it works." />;
  }

  return (
    <div className="relative pl-6">
      {/* Timeline line */}
      <div
        className="absolute left-2 top-0 bottom-0 w-0.5"
        style={{ backgroundColor: 'var(--accent)', opacity: 0.2 }}
      />

      <div className="space-y-3">
        {episodes.map((ep) => {
          const color = eventColors[ep.event_type] || 'var(--text-muted)';
          const isReflection = ep.event_type === 'reflection';
          const time = new Date(ep.created_at).toLocaleString();

          return (
            <div key={ep.id} className="relative">
              {/* Timeline dot */}
              <div
                className="absolute -left-[18px] top-3 w-2 h-2 rounded-full"
                style={{ backgroundColor: color }}
              />

              <div
                className="p-4 rounded-xl"
                style={{
                  backgroundColor: isReflection ? 'rgba(168, 85, 247, 0.04)' : 'var(--bg-secondary)',
                  borderLeft: isReflection ? '2px solid #a855f7' : '1px solid var(--border)',
                  borderTop: isReflection ? 'none' : '1px solid var(--border)',
                  borderRight: isReflection ? 'none' : '1px solid var(--border)',
                  borderBottom: isReflection ? 'none' : '1px solid var(--border)',
                  borderRadius: 'var(--radius-lg)',
                }}
              >
                <div className="flex items-center gap-2 mb-2">
                  <span className="text-[10px] text-[var(--text-muted)]">{time}</span>
                  <span
                    className="px-1.5 py-0.5 rounded text-[10px] font-medium"
                    style={{ backgroundColor: color + '15', color }}
                  >
                    {ep.event_type.replace('_', ' ')}
                  </span>
                  {ep.outcome && (
                    <span
                      className="text-[10px]"
                      style={{
                        color: ep.outcome === 'success' ? 'var(--status-active)' : 'var(--status-error)',
                      }}
                    >
                      {ep.outcome}
                    </span>
                  )}
                </div>
                <p className="text-sm text-[var(--text-secondary)] leading-relaxed">{ep.summary}</p>
                {ep.task_id && (
                  <span className="text-[10px] text-[var(--text-muted)] mt-1 inline-block">
                    task {ep.task_id.slice(0, 8)}
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
