import { invoke } from '@tauri-apps/api/core';
import type { Agent, Episode, AuditEntry, AppConfig } from './types';

export async function getAgents(): Promise<Agent[]> {
  return invoke<Agent[]>('get_agents');
}

export async function getAgent(id: string): Promise<Agent> {
  return invoke<Agent>('get_agent', { id });
}

export async function createAgent(
  name: string,
  systemPrompt: string,
  toolsAllowed: string[]
): Promise<Agent> {
  return invoke<Agent>('create_agent', {
    name,
    systemPrompt,
    toolsAllowed,
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

export async function getServerInfo(): Promise<{ port: number; host: string }> {
  return invoke<{ port: number; host: string }>('get_server_info');
}

export async function resetApp(): Promise<void> {
  return invoke<void>('reset_app');
}
