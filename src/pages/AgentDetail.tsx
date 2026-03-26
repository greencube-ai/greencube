import { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { getAgent, getEpisodes, getAuditLog } from '../lib/invoke';
import { onActivityUpdate } from '../lib/events';
import { StatusBadge } from '../components/StatusBadge';
import { MemoryViewer } from '../components/MemoryViewer';
import { AuditLog } from '../components/AuditLog';
import type { Agent, Episode, AuditEntry } from '../lib/types';

type Tab = 'overview' | 'memory' | 'audit';

export function AgentDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [agent, setAgent] = useState<Agent | null>(null);
  const [episodes, setEpisodes] = useState<Episode[]>([]);
  const [auditEntries, setAuditEntries] = useState<AuditEntry[]>([]);
  const [activeTab, setActiveTab] = useState<Tab>('overview');
  const [error, setError] = useState('');

  useEffect(() => {
    if (!id) return;
    Promise.all([getAgent(id), getEpisodes(id), getAuditLog(id)])
      .then(([a, e, au]) => {
        setAgent(a);
        setEpisodes(e);
        setAuditEntries(au);
      })
      .catch((e) => setError(String(e)));
  }, [id]);

  // Listen for activity updates for this agent
  useEffect(() => {
    if (!id) return;
    const unlisten = onActivityUpdate((entry) => {
      if (entry.agent_id === id) {
        setAuditEntries((prev) => [entry, ...prev].slice(0, 100));
        // Refresh agent data
        getAgent(id).then(setAgent).catch(console.error);
        getEpisodes(id).then(setEpisodes).catch(console.error);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [id]);

  if (error) {
    return (
      <div>
        <button onClick={() => navigate('/')} className="text-sm text-[var(--text-muted)] mb-4 hover:text-[var(--text-primary)]">
          Back to Dashboard
        </button>
        <p className="text-[var(--status-error)]">{error}</p>
      </div>
    );
  }

  if (!agent) {
    return <div className="text-[var(--text-muted)]">Loading...</div>;
  }

  const successRate = agent.total_tasks > 0
    ? Math.round((agent.successful_tasks / agent.total_tasks) * 100)
    : 0;

  const tabs: { key: Tab; label: string }[] = [
    { key: 'overview', label: 'Overview' },
    { key: 'memory', label: 'Memory' },
    { key: 'audit', label: 'Audit Log' },
  ];

  return (
    <div>
      <button
        onClick={() => navigate('/')}
        className="text-sm text-[var(--text-muted)] mb-4 hover:text-[var(--text-primary)]"
      >
        Back to Dashboard
      </button>

      {/* Agent header */}
      <div className="flex items-center gap-4 mb-6">
        <h1 className="text-xl font-bold">{agent.name}</h1>
        <StatusBadge status={agent.status} />
      </div>

      {/* Tabs */}
      <div className="flex gap-1 mb-6 border-b" style={{ borderColor: 'var(--border)' }}>
        {tabs.map((tab) => (
          <button
            key={tab.key}
            onClick={() => setActiveTab(tab.key)}
            className={`px-4 py-2 text-sm transition-colors border-b-2 -mb-px ${
              activeTab === tab.key
                ? 'text-[var(--accent)] border-[var(--accent)]'
                : 'text-[var(--text-muted)] border-transparent hover:text-[var(--text-primary)]'
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Tab content */}
      {activeTab === 'overview' && (
        <div>
          {/* Stats row */}
          <div className="grid grid-cols-4 gap-4 mb-6">
            {[
              { label: 'Tasks', value: agent.total_tasks },
              { label: 'Success Rate', value: `${successRate}%` },
              { label: 'Total Spend', value: `$${(agent.total_spend_cents / 100).toFixed(2)}` },
              { label: 'Reputation', value: agent.reputation.toFixed(2) },
            ].map((stat) => (
              <div
                key={stat.label}
                className="p-3 rounded-lg border"
                style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
              >
                <div className="text-xs text-[var(--text-muted)] mb-1">{stat.label}</div>
                <div className="text-lg font-bold">{stat.value}</div>
              </div>
            ))}
          </div>

          {/* Info */}
          <div className="grid grid-cols-2 gap-4 mb-6">
            <div>
              <div className="text-xs text-[var(--text-muted)] mb-1">Tools</div>
              <div className="flex flex-wrap gap-1">
                {agent.tools_allowed.map((tool) => (
                  <span
                    key={tool}
                    className="px-2 py-0.5 rounded text-xs border"
                    style={{ borderColor: 'var(--border)', color: 'var(--text-secondary)' }}
                  >
                    {tool}
                  </span>
                ))}
              </div>
            </div>
            <div>
              <div className="text-xs text-[var(--text-muted)] mb-1">Public Key</div>
              <div className="text-xs text-[var(--text-secondary)] font-mono truncate">
                {agent.public_key}
              </div>
            </div>
          </div>

          <div className="text-xs text-[var(--text-muted)] mb-1">Created</div>
          <div className="text-sm text-[var(--text-secondary)] mb-6">
            {new Date(agent.created_at).toLocaleString()}
          </div>

          {agent.system_prompt && (
            <>
              <div className="text-xs text-[var(--text-muted)] mb-1">System Prompt</div>
              <div
                className="text-sm p-3 rounded-lg border mb-6"
                style={{ backgroundColor: 'var(--bg-tertiary)', borderColor: 'var(--border)' }}
              >
                {agent.system_prompt}
              </div>
            </>
          )}

          {/* Recent activity */}
          <div className="text-xs text-[var(--text-muted)] mb-2">Recent Activity</div>
          <AuditLog entries={auditEntries.slice(0, 10)} />
        </div>
      )}

      {activeTab === 'memory' && <MemoryViewer episodes={episodes} />}
      {activeTab === 'audit' && <AuditLog entries={auditEntries} />}
    </div>
  );
}
