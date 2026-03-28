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
  provider_id?: string;
  dynamic_profile: string;
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
    memory_mode: string; // "off", "keyword"
    self_reflection_enabled: boolean;
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
    alive_mode: boolean;
  };
}

export interface KnowledgeEntry {
  id: string;
  agent_id: string;
  content: string;
  source_task_id?: string;
  category: string;
  confidence: number;
  created_at: string;
  last_used_at?: string;
  use_count: number;
}

export interface DockerStatus {
  available: boolean;
}

export interface Notification {
  id: string;
  agent_id: string;
  content: string;
  notification_type: string;
  read: boolean;
  created_at: string;
  source?: string;
}

export interface CreatureStatus {
  mood: string;
  top_strength?: [string, number];
  top_weakness?: [string, number];
  knowledge_count: number;
  last_reflection_summary?: string;
}

export interface CompetenceEntry {
  domain: string;
  confidence: number;
  task_count: number;
  success_count: number;
  trend: string;
  last_assessed: string;
}

export interface Provider {
  id: string;
  name: string;
  api_base_url: string;
  api_key: string;
  default_model: string;
  provider_type: string;
  created_at: string;
}
