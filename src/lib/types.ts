export interface Agent {
  id: string;
  name: string;
  created_at: string;
  updated_at: string;
  status: 'idle' | 'active' | 'error';
  system_prompt: string;
  tools_allowed: string[];
  max_spend_cents: number;
  total_tasks: number;
  successful_tasks: number;
  total_spend_cents: number;
  reputation: number;
  public_key: string;
}

export interface Episode {
  id: string;
  agent_id: string;
  created_at: string;
  event_type: string;
  summary: string;
  raw_data?: string;
  task_id?: string;
  outcome?: string;
  tokens_used: number;
  cost_cents: number;
}

export interface AuditEntry {
  id: string;
  agent_id: string;
  created_at: string;
  action_type: string;
  action_detail: string;
  permission_result: string;
  result?: string;
  duration_ms?: number;
  cost_cents: number;
  error?: string;
}

export interface AppConfig {
  llm: {
    api_base_url: string;
    api_key: string;
    default_model: string;
  };
  server: {
    host: string;
    port: number;
  };
  sandbox: {
    image: string;
    cpu_limit_cores: number;
    memory_limit_mb: number;
    timeout_seconds: number;
    network_enabled: boolean;
  };
  ui: {
    onboarding_complete: boolean;
  };
}

export interface DockerStatus {
  available: boolean;
}
