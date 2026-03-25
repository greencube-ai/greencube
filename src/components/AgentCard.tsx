import type { Agent } from '../lib/types';
import { StatusBadge } from './StatusBadge';

interface AgentCardProps {
  agent: Agent;
  onClick: () => void;
}

export function AgentCard({ agent, onClick }: AgentCardProps) {
  return (
    <div
      onClick={onClick}
      className="p-4 rounded-lg border cursor-pointer transition-colors hover:border-[var(--border-hover)]"
      style={{
        backgroundColor: 'var(--bg-secondary)',
        borderColor: 'var(--border)',
      }}
    >
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-base font-medium">{agent.name}</h3>
        <StatusBadge status={agent.status} />
      </div>

      <div className="flex items-center gap-4 text-xs text-[var(--text-muted)] mb-3">
        <span>{agent.tools_allowed.length} tools</span>
        <span>{agent.total_tasks} tasks</span>
        <span>
          {agent.total_tasks > 0
            ? Math.round((agent.successful_tasks / agent.total_tasks) * 100)
            : 0}
          % success
        </span>
      </div>

      {/* Reputation bar */}
      <div className="h-1 rounded-full" style={{ backgroundColor: 'var(--bg-tertiary)' }}>
        <div
          className="h-1 rounded-full transition-all"
          style={{
            width: `${agent.reputation * 100}%`,
            backgroundColor: 'var(--accent)',
          }}
        />
      </div>
    </div>
  );
}
