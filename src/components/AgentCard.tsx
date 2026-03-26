import type { Agent } from '../lib/types';
import { StatusBadge } from './StatusBadge';

interface AgentCardProps {
  agent: Agent;
  onClick: () => void;
}

export function AgentCard({ agent, onClick }: AgentCardProps) {
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
      <div className="flex items-center justify-between mb-4">
        <h3 className="text-base font-semibold">{agent.name}</h3>
        <StatusBadge status={agent.status} />
      </div>

      <div className="flex items-center gap-4 text-xs text-[var(--text-muted)] mb-4">
        <span>{agent.tools_allowed.length} tools</span>
        <span className="text-[var(--border)]">|</span>
        <span>{agent.total_tasks} tasks</span>
        <span className="text-[var(--border)]">|</span>
        <span>{successRate}% success</span>
      </div>

      {/* Reputation bar */}
      <div className="h-1.5 rounded-full" style={{ backgroundColor: 'var(--bg-tertiary)' }}>
        <div
          className="h-1.5 rounded-full transition-all duration-500"
          style={{
            width: `${Math.max(agent.reputation * 100, 2)}%`,
            backgroundColor: 'var(--accent)',
          }}
        />
      </div>
    </div>
  );
}
