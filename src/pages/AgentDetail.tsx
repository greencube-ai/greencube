import { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { getAgent, getAuditLog, getKnowledge, getCompetenceMap, getCreatureStatus } from '../lib/invoke';
import { StatusBadge } from '../components/StatusBadge';
import { KnowledgeList } from '../components/KnowledgeList';
import { AuditLog } from '../components/AuditLog';
import type { Agent, AuditEntry, KnowledgeEntry, CompetenceEntry, CreatureStatus } from '../lib/types';

type Tab = 'overview' | 'brain' | 'log';

export function AgentDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [agent, setAgent] = useState<Agent | null>(null);
  const [knowledge, setKnowledge] = useState<KnowledgeEntry[]>([]);
  const [auditEntries, setAuditEntries] = useState<AuditEntry[]>([]);
  const [competence, setCompetence] = useState<CompetenceEntry[]>([]);
  const [creatureStatus, setCreatureStatus] = useState<CreatureStatus | null>(null);
  const [activeTab, setActiveTab] = useState<Tab>('overview');
  const [error, setError] = useState('');

  useEffect(() => {
    if (!id) return;
    const fetchAll = () => {
      getAgent(id).then(setAgent).catch((e) => setError(String(e)));
      getKnowledge(id).then(setKnowledge).catch(console.error);
      getAuditLog(id).then(setAuditEntries).catch(console.error);
      getCompetenceMap(id).then(setCompetence).catch(console.error);
      getCreatureStatus(id).then(setCreatureStatus).catch(console.error);
    };
    fetchAll();
    const interval = setInterval(fetchAll, 5000);
    return () => clearInterval(interval);
  }, [id]);

  if (error) return <div><button onClick={() => navigate('/')} className="text-sm text-[var(--text-muted)] mb-4">Back</button><p className="text-[var(--status-error)]">{error}</p></div>;
  if (!agent) return <div className="text-[var(--text-muted)]">Loading...</div>;

  const successRate = agent.total_tasks > 0 ? Math.round((agent.successful_tasks / agent.total_tasks) * 100) : 0;

  const tabs: { key: Tab; label: string; count?: number }[] = [
    { key: 'overview', label: 'Overview' },
    { key: 'brain', label: 'Brain', count: knowledge.length },
    { key: 'log', label: 'Log', count: auditEntries.length },
  ];

  return (
    <div>
      <button onClick={() => navigate('/')} className="text-sm text-[var(--text-muted)] mb-6 hover:text-[var(--text-primary)]">Back</button>

      <div className="flex items-start justify-between mb-6">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-bold">{agent.name}</h1>
          <StatusBadge status={agent.status} />
          {creatureStatus && (
            <span className="text-xs px-2 py-0.5 rounded-full" style={{
              color: creatureStatus.mood === 'thriving' ? 'var(--accent)' : creatureStatus.mood === 'struggling' ? 'var(--status-error)' : 'var(--text-muted)',
              backgroundColor: creatureStatus.mood === 'thriving' ? 'var(--accent-subtle)' : 'var(--bg-tertiary)',
            }}>
              {creatureStatus.mood}
            </span>
          )}
        </div>
        <span className="text-xs text-[var(--text-muted)]">{agent.reputation.toFixed(2)} rep</span>
      </div>

      <div className="flex gap-0 mb-8 border-b" style={{ borderColor: 'var(--border)' }}>
        {tabs.map((tab) => (
          <button key={tab.key} onClick={() => setActiveTab(tab.key)}
            className={`px-5 py-3 text-sm font-medium border-b-2 -mb-px transition-colors ${
              activeTab === tab.key ? 'text-[var(--accent)] border-[var(--accent)]' : 'text-[var(--text-muted)] border-transparent hover:text-[var(--text-primary)]'
            }`}>
            {tab.label}
            {tab.count !== undefined && tab.count > 0 && (
              <span className="ml-2 text-[10px] px-1.5 py-0.5 rounded-full" style={{ backgroundColor: 'var(--bg-tertiary)', color: 'var(--text-muted)' }}>{tab.count}</span>
            )}
          </button>
        ))}
      </div>

      {activeTab === 'overview' && (
        <div>
          {/* Creature inner life */}
          {creatureStatus && (creatureStatus.active_domain || creatureStatus.recent_insight || creatureStatus.pending_investigation) && (
            <div className="mb-6 p-4 rounded-xl border text-xs" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
              <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-2">state of mind</div>
              <div className="space-y-1 text-[var(--text-secondary)]">
                {creatureStatus.active_domain && (
                  <div>Last worked on: <span className="text-[var(--text-primary)] font-medium">{creatureStatus.active_domain}</span></div>
                )}
                {creatureStatus.recent_insight && (
                  <div>Noticed: <span className="text-[var(--text-primary)]">{creatureStatus.recent_insight.length > 120 ? creatureStatus.recent_insight.slice(0, 120) + '...' : creatureStatus.recent_insight}</span></div>
                )}
                {creatureStatus.pending_investigation && (
                  <div>Curious about: <span style={{ color: '#06b6d4' }}>{creatureStatus.pending_investigation}</span></div>
                )}
                {creatureStatus.top_strength && (
                  <div>Strength: <span style={{ color: 'var(--accent)' }}>{creatureStatus.top_strength[0]} ({Math.round(creatureStatus.top_strength[1] * 100)}%)</span>
                    {creatureStatus.top_weakness && creatureStatus.top_weakness[0] !== creatureStatus.top_strength[0] && (
                      <span className="text-[var(--text-muted)]"> · Weakness: <span style={{ color: 'var(--status-error)' }}>{creatureStatus.top_weakness[0]} ({Math.round(creatureStatus.top_weakness[1] * 100)}%)</span></span>
                    )}
                  </div>
                )}
              </div>
            </div>
          )}

          {/* Stats */}
          <div className="grid grid-cols-2 gap-4 mb-8 max-w-md">
            <div className="p-4 rounded-xl border" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
              <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-2">Tasks</div>
              <div className="text-3xl font-bold">{agent.total_tasks}</div>
            </div>
            <div className="p-4 rounded-xl border" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
              <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-2">Success</div>
              <div className="text-3xl font-bold" style={{ color: successRate >= 80 ? 'var(--accent)' : successRate >= 50 ? '#eab308' : 'var(--status-error)' }}>{successRate}%</div>
            </div>
          </div>

          {/* Competence */}
          {competence.length > 0 && (
            <div className="mb-8">
              <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-3">Competence</div>
              <div className="space-y-2">
                {competence.map((c) => {
                  const pct = Math.round(c.confidence * 100);
                  const color = pct >= 80 ? 'var(--accent)' : pct >= 50 ? '#eab308' : 'var(--status-error)';
                  return (
                    <div key={c.domain} className="flex items-center gap-3">
                      <span className="text-xs font-medium w-20 text-right text-[var(--text-secondary)]">{c.domain}</span>
                      <div className="flex-1 h-2 rounded-full" style={{ backgroundColor: 'var(--bg-tertiary)' }}>
                        <div className="h-2 rounded-full" style={{ width: `${Math.max(pct, 2)}%`, backgroundColor: color }} />
                      </div>
                      <span className="text-[10px] w-16 text-[var(--text-muted)]">{pct}% ({c.task_count})</span>
                    </div>
                  );
                })}
              </div>
            </div>
          )}

          {/* Recent knowledge */}
          {knowledge.length > 0 && (
            <div className="mb-8">
              <div className="flex items-center justify-between mb-3">
                <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide">What it knows</div>
                <button onClick={() => setActiveTab('brain')} className="text-[10px] text-[var(--accent)]">View all</button>
              </div>
              <KnowledgeList entries={knowledge.slice(0, 5)} />
            </div>
          )}
        </div>
      )}

      {activeTab === 'brain' && <KnowledgeList entries={knowledge} />}
      {activeTab === 'log' && <AuditLog entries={auditEntries} />}
    </div>
  );
}
