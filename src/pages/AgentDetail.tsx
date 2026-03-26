import { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { getAgent, getEpisodes, getAuditLog, getKnowledge, getAgentContext } from '../lib/invoke';
import { onActivityUpdate } from '../lib/events';
import { StatusBadge } from '../components/StatusBadge';
import { MemoryViewer } from '../components/MemoryViewer';
import { AuditLog } from '../components/AuditLog';
import { KnowledgeList } from '../components/KnowledgeList';
import type { Agent, Episode, AuditEntry, KnowledgeEntry } from '../lib/types';

type Tab = 'overview' | 'brain' | 'memory' | 'log';

export function AgentDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [agent, setAgent] = useState<Agent | null>(null);
  const [episodes, setEpisodes] = useState<Episode[]>([]);
  const [auditEntries, setAuditEntries] = useState<AuditEntry[]>([]);
  const [knowledge, setKnowledge] = useState<KnowledgeEntry[]>([]);
  const [context, setContext] = useState('');
  const [activeTab, setActiveTab] = useState<Tab>('overview');
  const [error, setError] = useState('');

  useEffect(() => {
    if (!id) return;
    getAgent(id).then(setAgent).catch((e) => setError(String(e)));
    getEpisodes(id).then(setEpisodes).catch(console.error);
    getAuditLog(id).then(setAuditEntries).catch(console.error);
    getKnowledge(id).then(setKnowledge).catch(console.error);
    getAgentContext(id).then(setContext).catch(console.error);
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
          Back to Habitat
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
    { key: 'brain', label: 'Brain', count: knowledge.length },
    { key: 'memory', label: 'Memory', count: episodes.length },
    { key: 'log', label: 'Log', count: auditEntries.length },
  ];

  const reflections = episodes.filter(e => e.event_type === 'reflection');
  const lastReflection = reflections.length > 0 ? reflections[0] : null;

  return (
    <div>
      <button
        onClick={() => navigate('/')}
        className="text-sm text-[var(--text-muted)] mb-6 hover:text-[var(--text-primary)] transition-colors inline-block"
      >
        Back to Habitat
      </button>

      {/* Identity header */}
      <div className="flex items-start justify-between mb-8">
        <div>
          <div className="flex items-center gap-3 mb-2">
            <h1 className="text-2xl font-bold">{agent.name}</h1>
            <StatusBadge status={agent.status} />
          </div>
          {agent.system_prompt && (
            <p className="text-sm text-[var(--text-muted)] max-w-lg leading-relaxed">
              {agent.system_prompt.slice(0, 150)}{agent.system_prompt.length > 150 ? '...' : ''}
            </p>
          )}
        </div>
        <div className="text-right text-xs text-[var(--text-muted)] flex-shrink-0">
          <div>{new Date(agent.created_at).toLocaleDateString()}</div>
          <div className="font-mono mt-1 text-[var(--text-secondary)]">{agent.reputation.toFixed(2)} rep</div>
        </div>
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

      {/* OVERVIEW — the athlete profile */}
      {activeTab === 'overview' && (
        <div>
          {/* Vital signs */}
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 mb-8">
            {[
              { label: 'Tasks', value: agent.total_tasks, color: 'var(--text-primary)' },
              { label: 'Success', value: `${successRate}%`, color: successRate >= 80 ? 'var(--accent)' : successRate >= 50 ? '#eab308' : 'var(--status-error)' },
              { label: 'Knowledge', value: knowledge.length, color: knowledge.length > 0 ? 'var(--accent)' : 'var(--text-muted)' },
              { label: 'Spend', value: `$${(agent.total_spend_cents / 100).toFixed(2)}`, color: 'var(--text-secondary)' },
            ].map((stat) => (
              <div
                key={stat.label}
                className="p-4 rounded-xl border"
                style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
              >
                <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-2">{stat.label}</div>
                <div className="text-2xl font-bold" style={{ color: stat.color }}>{stat.value}</div>
              </div>
            ))}
          </div>

          {/* Capabilities */}
          <div className="mb-8">
            <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-3">Capabilities</div>
            <div className="flex flex-wrap gap-2">
              {agent.tools_allowed.map((tool) => (
                <span
                  key={tool}
                  className="px-3 py-1.5 rounded-lg text-xs font-medium"
                  style={{ backgroundColor: 'var(--bg-tertiary)', color: 'var(--text-secondary)' }}
                >
                  {tool}
                </span>
              ))}
            </div>
          </div>

          {/* Scratchpad — what the agent is thinking about */}
          {context && (
            <div className="mb-8">
              <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-3">Working Notes</div>
              <div
                className="p-4 rounded-xl border font-mono text-xs leading-relaxed whitespace-pre-wrap"
                style={{ backgroundColor: 'var(--bg-tertiary)', borderColor: 'var(--border)', color: 'var(--text-secondary)' }}
              >
                {context}
              </div>
            </div>
          )}

          {/* Last reflection */}
          {lastReflection && (
            <div className="mb-8">
              <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-3">Last Reflection</div>
              <div
                className="p-4 rounded-xl border-l-2"
                style={{ backgroundColor: 'var(--bg-secondary)', borderColor: '#a855f7' }}
              >
                <p className="text-sm text-[var(--text-secondary)] leading-relaxed">
                  {(lastReflection.raw_data || lastReflection.summary).slice(0, 500)}
                </p>
                <div className="text-[10px] text-[var(--text-muted)] mt-3">
                  {new Date(lastReflection.created_at).toLocaleString()}
                </div>
              </div>
            </div>
          )}

          {/* Recent knowledge preview */}
          {knowledge.length > 0 && (
            <div className="mb-8">
              <div className="flex items-center justify-between mb-3">
                <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide">Recent Knowledge</div>
                <button
                  onClick={() => setActiveTab('brain')}
                  className="text-[10px] text-[var(--accent)] hover:underline"
                >
                  View all
                </button>
              </div>
              <KnowledgeList entries={knowledge.slice(0, 3)} />
            </div>
          )}

          {/* Recent activity */}
          <div>
            <div className="flex items-center justify-between mb-3">
              <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide">Recent Activity</div>
              <button
                onClick={() => setActiveTab('log')}
                className="text-[10px] text-[var(--accent)] hover:underline"
              >
                View all
              </button>
            </div>
            <div
              className="rounded-xl border p-4"
              style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
            >
              <AuditLog entries={auditEntries.slice(0, 8)} />
            </div>
          </div>
        </div>
      )}

      {/* BRAIN — all knowledge */}
      {activeTab === 'brain' && <KnowledgeList entries={knowledge} />}

      {/* MEMORY — episode timeline */}
      {activeTab === 'memory' && <MemoryViewer episodes={episodes} />}

      {/* LOG — full audit trail */}
      {activeTab === 'log' && <AuditLog entries={auditEntries} />}
    </div>
  );
}
