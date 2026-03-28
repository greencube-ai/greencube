import type { Agent } from '../lib/types';
import { StatusBadge } from './StatusBadge';

interface AgentCardProps {
  agent: Agent;
  onClick: () => void;
}

function timeAgo(dateStr: string): string {
  const diff = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

export function AgentCard({ agent, onClick }: AgentCardProps) {
  const successRate = agent.total_tasks > 0 ? Math.round((agent.successful_tasks / agent.total_tasks) * 100) : 0;
  const staleSecs = (Date.now() - new Date(agent.updated_at).getTime()) / 1000;
  const displayStatus = (agent.status === 'active' && staleSecs > 30) ? 'idle' : agent.status;

  return (
    <div onClick={onClick} className="p-5 rounded-xl border cursor-pointer transition-all duration-200 hover:translate-y-[-1px]"
      style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
      onMouseEnter={(e) => { e.currentTarget.style.borderColor = 'var(--accent)'; }}
      onMouseLeave={(e) => { e.currentTarget.style.borderColor = 'var(--border)'; }}>
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-base font-semibold">{agent.name}</h3>
        <StatusBadge status={displayStatus} />
      </div>
      <div className="text-xs text-[var(--text-muted)] mb-3">
        {agent.total_tasks === 0 ? 'waiting for first task' : (
          <>{agent.total_tasks} tasks / <span style={{ color: successRate >= 80 ? 'var(--accent)' : successRate >= 50 ? '#eab308' : 'var(--status-error)' }}>{successRate}%</span></>
        )}
      </div>
      <div className="text-[10px] text-[var(--text-muted)]">{timeAgo(agent.updated_at)}</div>
    </div>
  );
}
