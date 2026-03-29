import type { KnowledgeEntry } from '../lib/types';
import { EmptyState } from './EmptyState';

interface KnowledgeListProps {
  entries: KnowledgeEntry[];
}

const categoryColors: Record<string, string> = {
  fact: '#3b82f6',
  warning: '#ef4444',
  preference: '#a855f7',
  skill: '#22c55e',
  pattern: '#eab308',
};

export function KnowledgeList({ entries }: KnowledgeListProps) {
  if (entries.length === 0) {
    return (
      <EmptyState
        message="No knowledge yet"
        subtitle="Send a few messages. Knowledge gets extracted automatically after tasks."
      />
    );
  }

  return (
    <div className="space-y-3">
      {entries.map((entry) => {
        const color = categoryColors[entry.category] || 'var(--text-muted)';
        return (
          <div
            key={entry.id}
            className="p-4 rounded-xl border"
            style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
          >
            <div className="flex items-start justify-between gap-3 mb-2">
              <span
                className="px-2 py-0.5 rounded text-[10px] font-medium flex-shrink-0"
                style={{ backgroundColor: color + '15', color }}
              >
                {entry.category}
              </span>
              <span className="text-[10px] text-[var(--text-muted)] flex-shrink-0">
                {new Date(entry.created_at).toLocaleDateString()}
              </span>
            </div>
            <p className="text-sm text-[var(--text-secondary)] leading-relaxed">
              {entry.content}
            </p>
            <div className="flex items-center gap-3 mt-3">
              <div className="flex items-center gap-1.5">
                <span className="text-[10px] text-[var(--text-muted)]">confidence</span>
                <div className="w-16 h-1 rounded-full" style={{ backgroundColor: 'var(--bg-tertiary)' }}>
                  <div
                    className="h-1 rounded-full"
                    style={{ width: `${entry.confidence * 100}%`, backgroundColor: color }}
                  />
                </div>
              </div>
              {entry.use_count > 0 && (
                <span className="text-[10px] text-[var(--text-muted)]">
                  used {entry.use_count}x
                </span>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
