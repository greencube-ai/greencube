import { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { getAgent, getEpisodes, getAuditLog, getKnowledge } from '../lib/invoke';
import { onActivityUpdate } from '../lib/events';
import { StatusBadge } from '../components/StatusBadge';
import { MemoryViewer } from '../components/MemoryViewer';
import { AuditLog } from '../components/AuditLog';
import { KnowledgeList } from '../components/KnowledgeList';
import type { Agent, Episode, AuditEntry, KnowledgeEntry } from '../lib/types';

type Tab = 'overview' | 'memory' | 'knowledge' | 'audit';

export function AgentDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [agent, setAgent] = useState<Agent | null>(null);
  const [episodes, setEpisodes] = useState<Episode[]>([]);
  const [auditEntries, setAuditEntries] = useState<AuditEntry[]>([]);
  const [knowledge, setKnowledge] = useState<KnowledgeEntry[]>([]);
  const [activeTab, setActiveTab] = useState<Tab>('overview');
  const [error, setError] = useState('');

  useEffect(() => {
    if (!id) return;
    Promise.all([getAgent(id), getEpisodes(id), getAuditLog(id), getKnowledge(id)])
      .then(([a, e, au, k]) => {
        setAgent(a);
        setEpisodes(e);
        setAuditEntries(au);
        setKnowledge(k);
      })
      .catch((e) => setError(String(e)));
  }, [id]);

  useEffect(() => {
    if (!id) return;
    const unlisten = onActivityUpdate((entry) => {
      if (entry.agent_id === id) {
        setAuditEntries((prev) => [entry, ...prev].slice(0, 100));
        getAgent(id).then(setAgent).catch(console.error);
        getEpisodes(id).then(setEpisodes).catch(console.error);
        getKnowledge(id).then(setKnowledge).catch(console.error);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [id]);

  if (error) {
    return (
      <div className="py-8">
        <button onClick={() => navigate('/')} className="text-sm text-[var(--text-muted)] mb-4 hover:text-[var(--text-primary)] transition-colors">
          Back to Dashboard
        </button>
        <p className="text-[var(--status-error)]">{error}</p>
      </div>
    );
  }

  if (!agent) {
    return <div className="py-8 text-[var(--text-muted)]">Loading...</div>;
  }

  const successRate = agent.total_tasks > 0
    ? Math.round((agent.successful_tasks / agent.total_tasks) * 100)
    : 0;

  const tabs: { key: Tab; label: string; count?: number }[] = [
    { key: 'overview', label: 'Overview' },
    { key: 'memory', label: 'Memory', count: episodes.length },
    { key: 'knowledge', label: 'Knowledge', count: knowledge.length },
    { key: 'audit', label: 'Audit Log', count: auditEntries.length },
  ];

  const stats = [
    { label: 'Tasks', value: agent.total_tasks, color: 'var(--accent)' },
    { label: 'Success', value: `${successRate}%`, color: successRate >= 80 ? 'var(--accent)' : successRate >= 50 ? '#eab308' : 'var(--status-error)' },
    { label: 'Spend', value: `$${(agent.total_spend_cents / 100).toFixed(2)}`, color: 'var(--text-secondary)' },
    { label: 'Reputation', value: agent.reputation.toFixed(2), color: 'var(--accent)' },
  ];

  return (
    <div>
      <button
        onClick={() => navigate('/')}
        className="text-sm text-[var(--text-muted)] mb-6 hover:text-[var(--text-primary)] transition-colors inline-block"
      >
        Back to Dashboard
      </button>

      {/* Agent header */}
      <div className="flex items-center gap-4 mb-8">
        <h1 className="text-2xl font-bold">{agent.name}</h1>
        <StatusBadge status={agent.status} />
      </div>

      {/* Tabs */}
      <div className="flex gap-0 mb-8 border-b" style={{ borderColor: 'var(--border)' }}>
        {tabs.map((tab) => (
          <button
            key={tab.key}
            onClick={() => setActiveTab(tab.key)}
            className={`px-5 py-3 text-sm transition-colors border-b-2 -mb-px font-medium ${
              activeTab === tab.key
                ? 'text-[var(--accent)] border-[var(--accent)]'
                : 'text-[var(--text-muted)] border-transparent hover:text-[var(--text-primary)]'
            }`}
          >
            {tab.label}
            {tab.count !== undefined && tab.count > 0 && (
              <span className="ml-2 text-[10px] px-1.5 py-0.5 rounded-full" style={{ backgroundColor: 'var(--bg-tertiary)', color: 'var(--text-muted)' }}>
                {tab.count}
              </span>
            )}
          </button>
        ))}
      </div>

      {/* Tab content */}
      {activeTab === 'overview' && (
        <div>
          {/* Stats row */}
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 mb-8">
            {stats.map((stat) => (
              <div
                key={stat.label}
                className="p-4 rounded-xl border"
                style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
              >
                <div className="text-xs text-[var(--text-muted)] mb-2">{stat.label}</div>
                <div className="text-2xl font-bold" style={{ color: stat.color }}>{stat.value}</div>
              </div>
            ))}
          </div>

          {/* Info grid */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-6 mb-8">
            <div>
              <div className="text-xs text-[var(--text-muted)] mb-2 uppercase tracking-wide">Tools</div>
              <div className="flex flex-wrap gap-2">
                {agent.tools_allowed.map((tool) => (
                  <span
                    key={tool}
                    className="px-3 py-1 rounded-lg text-xs font-medium"
                    style={{ backgroundColor: 'var(--bg-tertiary)', color: 'var(--text-secondary)' }}
                  >
                    {tool}
                  </span>
                ))}
              </div>
            </div>
            <div>
              <div className="text-xs text-[var(--text-muted)] mb-2 uppercase tracking-wide">Created</div>
              <div className="text-sm text-[var(--text-secondary)]">
                {new Date(agent.created_at).toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' })}
              </div>
            </div>
          </div>

          {agent.system_prompt && (
            <div className="mb-8">
              <div className="text-xs text-[var(--text-muted)] mb-2 uppercase tracking-wide">System Prompt</div>
              <div
                className="text-sm p-4 rounded-xl border leading-relaxed"
                style={{ backgroundColor: 'var(--bg-tertiary)', borderColor: 'var(--border)', color: 'var(--text-secondary)' }}
              >
                {agent.system_prompt}
              </div>
            </div>
          )}

          {/* Recent activity */}
          <div className="text-xs text-[var(--text-muted)] mb-3 uppercase tracking-wide">Recent Activity</div>
          <div
            className="rounded-xl border p-4"
            style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
          >
            <AuditLog entries={auditEntries.slice(0, 10)} />
          </div>
        </div>
      )}

      {activeTab === 'memory' && <MemoryViewer episodes={episodes} />}
      {activeTab === 'knowledge' && <KnowledgeList entries={knowledge} />}
      {activeTab === 'audit' && <AuditLog entries={auditEntries} />}
    </div>
  );
}
