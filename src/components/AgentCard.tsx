import type { Agent } from '../lib/types';
import { StatusBadge } from './StatusBadge';

interface AgentCardProps {
  agent: Agent;
  onClick: () => void;
  parentName?: string;
  childCount?: number;
}

function timeAgo(dateStr: string): string {
  const now = Date.now();
  const then = new Date(dateStr).getTime();
  const diff = Math.floor((now - then) / 1000);
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

export function AgentCard({ agent, onClick, parentName, childCount }: AgentCardProps) {
  const successRate = agent.total_tasks > 0
    ? Math.round((agent.successful_tasks / agent.total_tasks) * 100)
    : 0;

  return (
    <div
      onClick={onClick}
      className="p-5 rounded-xl border cursor-pointer transition-all duration-200 hover:translate-y-[-1px]"
      style={{
        backgroundColor: 'var(--bg-secondary)',
        borderColor: 'var(--border)',
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.borderColor = 'var(--accent)';
        e.currentTarget.style.boxShadow = '0 4px 12px rgba(34, 197, 94, 0.08)';
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.borderColor = 'var(--border)';
        e.currentTarget.style.boxShadow = 'none';
      }}
    >
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          <h3 className="text-base font-semibold">{agent.name}</h3>
          {parentName && (
            <span className="px-1.5 py-0.5 rounded text-[9px] font-medium" style={{ backgroundColor: 'rgba(168, 85, 247, 0.1)', color: '#a855f7' }}>
              from {parentName}
            </span>
          )}
          {childCount !== undefined && childCount > 0 && (
            <span className="px-1.5 py-0.5 rounded text-[9px] font-medium" style={{ backgroundColor: 'var(--accent-subtle)', color: 'var(--accent)' }}>
              {childCount} specialist{childCount > 1 ? 's' : ''}
            </span>
          )}
        </div>
        <StatusBadge status={agent.status} />
      </div>

      {/* Stats */}
      <div className="flex items-center gap-3 text-xs text-[var(--text-muted)] mb-3">
        <span>{agent.total_tasks} tasks</span>
        <span className="text-[var(--border)]">|</span>
        <span style={{ color: successRate >= 80 ? 'var(--accent)' : successRate >= 50 ? '#eab308' : 'var(--status-error)' }}>
          {successRate}%
        </span>
        <span className="text-[var(--border)]">|</span>
        <span>{agent.tools_allowed.length} tools</span>
      </div>

      <div className="text-[10px] text-[var(--text-muted)]">
        {timeAgo(agent.updated_at)}
      </div>

      {/* Reputation bar */}
      <div className="h-1 rounded-full mt-3" style={{ backgroundColor: 'var(--bg-tertiary)' }}>
        <div
          className="h-1 rounded-full transition-all duration-500"
          style={{
            width: `${Math.max(agent.reputation * 100, 2)}%`,
            backgroundColor: 'var(--accent)',
          }}
        />
      </div>
    </div>
  );
}
