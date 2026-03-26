import { useState, useEffect } from 'react';
import { useApp } from '../context/AppContext';
import { getServerInfo } from '../lib/invoke';
import { ChatPanel } from '../components/ChatPanel';

type Tab = 'openclaw' | 'python' | 'javascript' | 'curl' | 'langchain' | 'crewai';

export function Connect() {
  const { state } = useApp();
  const [port, setPort] = useState(9000);
  const [selectedAgentId, setSelectedAgentId] = useState('');
  const [activeTab, setActiveTab] = useState<Tab>('python');
  const [testResult, setTestResult] = useState<{ request: string; response: string } | null>(null);
  const [testing, setTesting] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    getServerInfo().then((info) => setPort(info.port)).catch(() => {});
  }, []);

  useEffect(() => {
    if (state.agents.length > 0 && !selectedAgentId) {
      setSelectedAgentId(state.agents[0].id);
    }
  }, [state.agents, selectedAgentId]);

  const snippets: Record<Tab, { label: string; code: string }> = {
    openclaw: {
      label: 'OpenClaw',
      code: `# Option 1: Environment variable
export OPENAI_API_BASE=http://localhost:${port}/v1
export OPENAI_API_KEY=any

# Then run your OpenClaw agent normally.
# All requests route through GreenCube automatically.

# Option 2: In OpenClaw config
# Set the model endpoint to:
#   http://localhost:${port}/v1/chat/completions
#
# GreenCube is OpenAI-compatible so any framework
# that supports custom API endpoints works.

# Option 3: In Python with OpenClaw
import os
os.environ["OPENAI_API_BASE"] = "http://localhost:${port}/v1"
os.environ["OPENAI_API_KEY"] = "any"

# Then import and use OpenClaw as normal.
# Your agent now has memory, sandbox, and audit.`,
    },
    python: {
      label: 'Python',
      code: `from openai import OpenAI

client = OpenAI(
    api_key="any",  # GreenCube doesn't check this
    base_url="http://localhost:${port}/v1"
)

response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Hello!"}],
    extra_headers={"x-agent-id": "${selectedAgentId}"}
)

print(response.choices[0].message.content)`,
    },
    javascript: {
      label: 'JavaScript',
      code: `import OpenAI from 'openai';

const client = new OpenAI({
  apiKey: 'any',
  baseURL: 'http://localhost:${port}/v1',
});

const response = await client.chat.completions.create({
  model: 'gpt-4o',
  messages: [{ role: 'user', content: 'Hello!' }],
});

console.log(response.choices[0].message.content);`,
    },
    curl: {
      label: 'curl',
      code: `curl http://localhost:${port}/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -H "x-agent-id: ${selectedAgentId}" \\
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'`,
    },
    langchain: {
      label: 'LangChain',
      code: `from langchain_openai import ChatOpenAI

llm = ChatOpenAI(
    model="gpt-4o",
    openai_api_key="any",
    openai_api_base="http://localhost:${port}/v1",
    default_headers={"x-agent-id": "${selectedAgentId}"}
)

response = llm.invoke("Hello!")
print(response.content)`,
    },
    crewai: {
      label: 'CrewAI',
      code: `import os
os.environ["OPENAI_API_KEY"] = "any"
os.environ["OPENAI_API_BASE"] = "http://localhost:${port}/v1"

from crewai import Agent, Task, Crew

agent = Agent(
    role="Helper",
    goal="Help the user",
    backstory="You are helpful.",
)

# CrewAI will route through GreenCube automatically`,
    },
  };

  const handleCopy = (code: string) => {
    navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleTest = async () => {
    if (!selectedAgentId) return;
    setTesting(true);
    setTestResult(null);
    const requestBody = {
      model: 'gpt-4o',
      messages: [{ role: 'user', content: 'Hello, are you there?' }],
    };
    const requestStr = JSON.stringify(requestBody, null, 2);
    try {
      const resp = await fetch(`http://localhost:${port}/v1/chat/completions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'x-agent-id': selectedAgentId },
        body: JSON.stringify(requestBody),
      });
      const data = await resp.json();
      setTestResult({ request: requestStr, response: JSON.stringify(data, null, 2) });
    } catch (e) {
      setTestResult({ request: requestStr, response: `Error: ${e}` });
    } finally {
      setTesting(false);
    }
  };

  // OpenClaw is shown as a hero section above. Other frameworks in tabs below.

  const otherTabs: Tab[] = ['python', 'javascript', 'curl', 'langchain', 'crewai'];

  return (
    <div className="max-w-3xl">
      <h1 className="text-2xl font-bold mb-2">Connect</h1>
      <p className="text-sm text-[var(--text-muted)] mb-8">
        One line. Your agent gets memory, safety, and an audit trail.
      </p>

      {/* Step-by-step: from zero to running */}
      <div
        className="rounded-xl border p-6 mb-8"
        style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
      >
        <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-4">Getting started</div>

        <div className="space-y-5">
          {/* Step 1: Install OpenClaw */}
          <div className="flex gap-4">
            <div className="w-6 h-6 rounded-full flex items-center justify-center flex-shrink-0 text-xs font-bold" style={{ backgroundColor: 'var(--accent-subtle)', color: 'var(--accent)' }}>1</div>
            <div className="flex-1">
              <div className="text-sm font-medium mb-1">Install OpenClaw</div>
              <p className="text-xs text-[var(--text-muted)] mb-2">If you don't have it yet:</p>
              <div className="rounded-lg p-3 font-mono text-xs" style={{ backgroundColor: 'var(--bg-primary)' }}>
                <span style={{ color: 'var(--text-secondary)' }}>pip install openclaw</span>
              </div>
            </div>
          </div>

          {/* Step 2: GreenCube is already running */}
          <div className="flex gap-4">
            <div className="w-6 h-6 rounded-full flex items-center justify-center flex-shrink-0 text-xs font-bold" style={{ backgroundColor: 'var(--accent-subtle)', color: 'var(--accent)' }}>2</div>
            <div className="flex-1">
              <div className="text-sm font-medium mb-1">GreenCube is already running</div>
              <p className="text-xs text-[var(--text-muted)]">
                You're looking at it. The API is live at <span className="font-mono" style={{ color: 'var(--accent)' }}>localhost:{port}</span>
              </p>
            </div>
          </div>

          {/* Step 3: Connect */}
          <div className="flex gap-4">
            <div className="w-6 h-6 rounded-full flex items-center justify-center flex-shrink-0 text-xs font-bold" style={{ backgroundColor: 'var(--accent-subtle)', color: 'var(--accent)' }}>3</div>
            <div className="flex-1">
              <div className="text-sm font-medium mb-2">Add one line before running OpenClaw</div>
              <div className="relative rounded-lg overflow-hidden" style={{ backgroundColor: 'var(--bg-primary)' }}>
                <button
                  onClick={() => handleCopy(`export OPENAI_API_BASE=http://localhost:${port}/v1`)}
                  className="absolute top-2 right-2 px-2 py-1 rounded text-[10px] border transition-colors"
                  style={{ borderColor: 'var(--border)', color: 'var(--text-muted)', backgroundColor: 'var(--bg-secondary)' }}
                >
                  {copied ? 'Copied!' : 'Copy'}
                </button>
                <pre className="p-4 text-sm font-mono" style={{ color: 'var(--accent)' }}>
{`export OPENAI_API_BASE=http://localhost:${port}/v1`}
                </pre>
              </div>
            </div>
          </div>

          {/* Step 4: Done */}
          <div className="flex gap-4">
            <div className="w-6 h-6 rounded-full flex items-center justify-center flex-shrink-0 text-xs font-bold" style={{ backgroundColor: 'var(--accent-subtle)', color: 'var(--accent)' }}>4</div>
            <div className="flex-1">
              <div className="text-sm font-medium mb-1">Run your agent</div>
              <p className="text-xs text-[var(--text-muted)]">
                That's it. Your agent now has persistent memory, an audit trail, and safe tool execution.
                Come back to the Habitat page to see it learning.
              </p>
            </div>
          </div>
        </div>
      </div>

      {/* Agent selector */}
      {state.agents.length > 0 && (
        <div className="mb-6">
          <label className="block text-xs text-[var(--text-muted)] mb-1">Agent</label>
          <select value={selectedAgentId} onChange={(e) => setSelectedAgentId(e.target.value)} className="w-64">
            {state.agents.map((a) => (
              <option key={a.id} value={a.id}>{a.name}</option>
            ))}
          </select>
        </div>
      )}

      {/* Other frameworks — smaller, below */}
      <div className="text-xs text-[var(--text-muted)] uppercase tracking-wide mb-3">Other frameworks</div>
      <div className="flex gap-1 mb-4 border-b" style={{ borderColor: 'var(--border)' }}>
        {otherTabs.map((tab) => (
          <button
            key={tab}
            onClick={() => setActiveTab(tab)}
            className={`px-3 py-2 text-xs transition-colors border-b-2 -mb-px ${
              activeTab === tab
                ? 'text-[var(--accent)] border-[var(--accent)]'
                : 'text-[var(--text-muted)] border-transparent hover:text-[var(--text-primary)]'
            }`}
          >
            {snippets[tab].label}
          </button>
        ))}
      </div>

      {/* Code block */}
      <div className="relative rounded-lg border overflow-hidden mb-6" style={{ backgroundColor: 'var(--bg-tertiary)', borderColor: 'var(--border)' }}>
        <button
          onClick={() => handleCopy(snippets[activeTab].code)}
          className="absolute top-2 right-2 px-2 py-1 rounded text-[10px] border transition-colors"
          style={{ borderColor: 'var(--border)', color: 'var(--text-muted)', backgroundColor: 'var(--bg-secondary)' }}
        >
          {copied ? 'Copied!' : 'Copy'}
        </button>
        <pre className="p-4 text-xs font-mono overflow-x-auto text-[var(--text-primary)]">
          {snippets[activeTab].code}
        </pre>
      </div>

      {/* Test button */}
      <button
        onClick={handleTest}
        disabled={testing || !selectedAgentId}
        className="px-4 py-2 rounded-lg text-black text-sm font-medium disabled:opacity-50 hover:brightness-110 transition mb-6"
        style={{ backgroundColor: 'var(--accent)' }}
      >
        {testing ? 'Testing...' : 'Test Connection'}
      </button>

      {/* What just happened? */}
      {testResult && (
        <div className="rounded-lg border overflow-hidden" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
          <div className="px-4 py-2 border-b text-xs font-medium text-[var(--text-secondary)]" style={{ borderColor: 'var(--border)' }}>
            What just happened?
          </div>
          <div className="grid grid-cols-2 divide-x" style={{ borderColor: 'var(--border)' }}>
            <div className="p-3">
              <div className="text-[10px] text-[var(--text-muted)] mb-1 uppercase tracking-wide">Request</div>
              <pre className="text-[10px] font-mono text-[var(--text-secondary)] overflow-auto max-h-48">{testResult.request}</pre>
            </div>
            <div className="p-3">
              <div className="text-[10px] text-[var(--text-muted)] mb-1 uppercase tracking-wide">Response</div>
              <pre className="text-[10px] font-mono text-[var(--text-secondary)] overflow-auto max-h-48">{testResult.response}</pre>
            </div>
          </div>
        </div>
      )}

      {/* Interactive test — for developers who want to try a full conversation */}
      {state.agents.length > 0 && (
        <div className="mt-10 pt-8 border-t" style={{ borderColor: 'var(--border)' }}>
          <h2 className="text-base font-medium text-[var(--text-secondary)] mb-1">Interactive Test</h2>
          <p className="text-xs text-[var(--text-muted)] mb-4">
            Send messages directly to verify your agent is working through GreenCube.
          </p>
          <ChatPanel
            agents={state.agents}
            apiPort={port}
            hasApiKey={true}
          />
        </div>
      )}
    </div>
  );
}
