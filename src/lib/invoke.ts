import { invoke } from '@tauri-apps/api/core';
import type { Agent, Episode, AuditEntry, AppConfig, Provider, KnowledgeEntry, Notification, CompetenceEntry } from './types';

export async function getAgents(): Promise<Agent[]> {
  return invoke<Agent[]>('get_agents');
}

export async function getAgent(id: string): Promise<Agent> {
  return invoke<Agent>('get_agent', { id });
}

export async function createAgent(
  name: string,
  systemPrompt: string,
  toolsAllowed: string[],
  providerId?: string
): Promise<Agent> {
  return invoke<Agent>('create_agent', {
    name,
    systemPrompt,
    toolsAllowed,
    providerId: providerId || null,
  });
}

export async function getEpisodes(
  agentId: string,
  limit: number = 50
): Promise<Episode[]> {
  return invoke<Episode[]>('get_episodes', { agentId, limit });
}

export async function getAuditLog(
  agentId: string,
  limit: number = 50
): Promise<AuditEntry[]> {
  return invoke<AuditEntry[]>('get_audit_log', { agentId, limit });
}

export async function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>('get_config');
}

export async function saveConfig(config: AppConfig): Promise<void> {
  return invoke<void>('save_config', { config });
}

export async function getDockerStatus(): Promise<{ available: boolean }> {
  return invoke<{ available: boolean }>('get_docker_status');
}

export async function getActivityFeed(
  limit: number = 50
): Promise<AuditEntry[]> {
  return invoke<AuditEntry[]>('get_activity_feed', { limit });
}

export async function getKnowledge(agentId: string, limit: number = 50): Promise<KnowledgeEntry[]> {
  return invoke<KnowledgeEntry[]>('get_knowledge', { agentId, limit });
}

export async function getAgentContext(agentId: string): Promise<string> {
  return invoke<string>('get_agent_context', { agentId });
}

export interface AgentLineage {
  parent: { id: string; name: string; domain: string } | null;
  children: { id: string; name: string; domain: string; knowledge_transferred: number }[];
}

export async function getAgentLineage(agentId: string): Promise<AgentLineage> {
  return invoke<AgentLineage>('get_agent_lineage', { agentId });
}

export async function debugSpawn(agentId: string, domain: string): Promise<string> {
  return invoke<string>('debug_spawn', { agentId, domain });
}

export async function setAgentContext(agentId: string, content: string): Promise<void> {
  return invoke<void>('set_agent_context', { agentId, content });
}

export async function getServerInfo(): Promise<{ port: number; host: string }> {
  return invoke<{ port: number; host: string }>('get_server_info');
}

export async function resetApp(): Promise<void> {
  return invoke<void>('reset_app');
}

// Provider CRUD
export async function getProviders(): Promise<Provider[]> {
  return invoke<Provider[]>('get_providers');
}

export async function createProvider(
  name: string,
  apiBaseUrl: string,
  apiKey: string,
  defaultModel: string,
  providerType: string
): Promise<Provider> {
  return invoke<Provider>('create_provider', {
    name,
    apiBaseUrl,
    apiKey,
    defaultModel,
    providerType,
  });
}

export async function updateProvider(
  id: string,
  name: string,
  apiBaseUrl: string,
  apiKey: string,
  defaultModel: string,
  providerType: string
): Promise<void> {
  return invoke<void>('update_provider', {
    id,
    name,
    apiBaseUrl,
    apiKey,
    defaultModel,
    providerType,
  });
}

export async function deleteProvider(id: string): Promise<void> {
  return invoke<void>('delete_provider', { id });
}

export async function getCompetenceMap(agentId: string): Promise<CompetenceEntry[]> {
  return invoke<CompetenceEntry[]>('get_competence_map', { agentId });
}

// Notifications
export async function getUnreadCount(): Promise<number> {
  return invoke<number>('get_unread_count');
}

export async function getNotifications(unreadOnly: boolean, limit: number = 20): Promise<Notification[]> {
  return invoke<Notification[]>('get_notifications', { unreadOnly, limit });
}

export async function markNotificationRead(id: string): Promise<void> {
  return invoke<void>('mark_notification_read', { id });
}

export async function dismissAllNotifications(): Promise<void> {
  return invoke<void>('dismiss_all_notifications');
}
