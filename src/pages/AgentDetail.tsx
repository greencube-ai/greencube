import { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { getAgent, getEpisodes, getAuditLog, getKnowledge, getAgentContext, getAgentLineage, getCompetenceMap } from '../lib/invoke';
import type { AgentLineage } from '../lib/invoke';
import type { CompetenceEntry } from '../lib/types';
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
  const [lineage, setLineage] = useState<AgentLineage | null>(null);
  const [competence, setCompetence] = useState<CompetenceEntry[]>([]);
  const [activeTab, setActiveTab] = useState<Tab>('overview');
  const [error, setError] = useState('');

  useEffect(() => {
    if (!id) return;
    const fetchAll = () => {
      getAgent(id).then(setAgent).catch((e) => setError(String(e)));
      getEpisodes(id).then(setEpisodes).catch(console.error);
      getAuditLog(id).then(setAuditEntries).catch(console.error);
      getKnowledge(id).then(setKnowledge).catch(console.error);
      getAgentContext(id).then(setContext).catch(console.error);
      getAgentLineage(id).then(setLineage).catch(console.error);
      getCompetenceMap(id).then(setCompetence).catch(console.error);
    };
    fetchAll();
    const interval = setInterval(fetchAll, 5000);
    return () => clearInterval(interval);
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
          {lineage?.parent && (
            <p className="text-xs mb-1" style={{ color: '#a855f7' }}>
              Spawned from {lineage.parent.name} to specialize in {lineage.parent.domain}
            </p>
          )}
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
          {/* Vital signs — just the two that matter */}
          <div className="grid grid-cols-2 gap-4 mb-8 max-w-md">
            <div className="p-4 rounded-xl border" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
              <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-2">Tasks</div>
              <div className="text-3xl font-bold">{agent.total_tasks}</div>
            </div>
            <div className="p-4 rounded-xl border" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
              <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-2">Success Rate</div>
              <div className="text-3xl font-bold" style={{ color: successRate >= 80 ? 'var(--accent)' : successRate >= 50 ? '#eab308' : 'var(--status-error)' }}>
                {successRate}%
              </div>
            </div>
          </div>

          {/* Competence map — visual bars */}
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
                        <div className="h-2 rounded-full transition-all duration-500" style={{ width: `${Math.max(pct, 2)}%`, backgroundColor: color }} />
                      </div>
                      <span className="text-[10px] w-16 text-[var(--text-muted)]">{pct}% ({c.task_count})</span>
                    </div>
                  );
                })}
              </div>
            </div>
          )}

          {/* Capabilities — small, unobtrusive */}
          <div className="flex flex-wrap gap-1.5 mb-8">
            {agent.tools_allowed.map((tool) => (
              <span
                key={tool}
                className="px-2 py-0.5 rounded text-[10px]"
                style={{ backgroundColor: 'var(--bg-tertiary)', color: 'var(--text-muted)' }}
              >
                {tool}
              </span>
            ))}
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

          {/* Last reflection — show extracted insights as bullet points */}
          {lastReflection && (
            <div className="mb-8">
              <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-3">Last Reflection</div>
              <div
                className="p-4 rounded-xl border-l-2"
                style={{ backgroundColor: 'var(--bg-secondary)', borderColor: '#a855f7' }}
              >
                <ul className="space-y-1.5">
                  {(() => {
                    const raw = lastReflection.raw_data || lastReflection.summary;
                    const tags = ['[fact]', '[preference]', '[warning]', '[skill]', '[context]'];
                    const insights: string[] = [];
                    for (const line of raw.split('\n')) {
                      for (const tag of tags) {
                        const idx = line.indexOf(tag);
                        if (idx !== -1) {
                          const content = line.slice(idx + tag.length).trim();
                          if (content) insights.push(content);
                        }
                      }
                    }
                    if (insights.length === 0) {
                      return <li className="text-sm text-[var(--text-secondary)]">{lastReflection.summary}</li>;
                    }
                    return insights.map((insight, i) => (
                      <li key={i} className="text-sm text-[var(--text-secondary)] flex gap-2">
                        <span className="text-[var(--text-muted)] flex-shrink-0">-</span>
                        <span>{insight}</span>
                      </li>
                    ));
                  })()}
                </ul>
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

          {/* Lineage — children (specialists) */}
          {lineage && lineage.children.length > 0 && (
            <div className="mb-8">
              <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-3">Specialists</div>
              <div className="space-y-2">
                {lineage.children.map((child) => (
                  <div
                    key={child.id}
                    onClick={() => navigate(`/agent/${child.id}`)}
                    className="p-3 rounded-xl border flex items-center justify-between cursor-pointer hover:border-[var(--accent)] transition-colors"
                    style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium">{child.name}</span>
                      <span className="text-[9px] px-1.5 py-0.5 rounded" style={{ backgroundColor: 'rgba(168, 85, 247, 0.1)', color: '#a855f7' }}>
                        {child.domain}
                      </span>
                    </div>
                    <span className="text-[10px] text-[var(--text-muted)]">
                      {child.knowledge_transferred} facts inherited
                    </span>
                  </div>
                ))}
              </div>
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
