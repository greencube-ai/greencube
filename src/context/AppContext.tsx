import React, { createContext, useContext, useReducer, useEffect } from 'react';
import type { Agent, AppConfig } from '../lib/types';
import { getAgents, getConfig, getDockerStatus } from '../lib/invoke';

interface AppState {
  agents: Agent[];
  config: AppConfig | null;
  dockerAvailable: boolean;
  loading: boolean;
}

type Action =
  | { type: 'SET_AGENTS'; agents: Agent[] }
  | { type: 'SET_CONFIG'; config: AppConfig }
  | { type: 'SET_DOCKER'; available: boolean }
  | { type: 'SET_LOADING'; loading: boolean }
  | { type: 'ADD_AGENT'; agent: Agent }
  | { type: 'UPDATE_AGENT_STATUS'; id: string; status: string };

const initialState: AppState = {
  agents: [],
  config: null,
  dockerAvailable: false,
  loading: true,
};

function reducer(state: AppState, action: Action): AppState {
  switch (action.type) {
    case 'SET_AGENTS':
      return { ...state, agents: action.agents };
    case 'SET_CONFIG':
      return { ...state, config: action.config };
    case 'SET_DOCKER':
      return { ...state, dockerAvailable: action.available };
    case 'SET_LOADING':
      return { ...state, loading: action.loading };
    case 'ADD_AGENT':
      return { ...state, agents: [action.agent, ...state.agents] };
    case 'UPDATE_AGENT_STATUS':
      return {
        ...state,
        agents: state.agents.map((a) =>
          a.id === action.id
            ? { ...a, status: action.status as Agent['status'] }
            : a
        ),
      };
    default:
      return state;
  }
}

const AppContext = createContext<{
  state: AppState;
  dispatch: React.Dispatch<Action>;
  refreshAgents: () => Promise<void>;
}>({
  state: initialState,
  dispatch: () => {},
  refreshAgents: async () => {},
});

export function AppProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(reducer, initialState);

  const refreshAgents = async () => {
    try {
      const agents = await getAgents();
      dispatch({ type: 'SET_AGENTS', agents });
    } catch (err) {
      console.error('Failed to refresh agents:', err);
    }
  };

  useEffect(() => {
    async function init() {
      try {
        const [agents, config, docker] = await Promise.all([
          getAgents(),
          getConfig(),
          getDockerStatus(),
        ]);
        dispatch({ type: 'SET_AGENTS', agents });
        dispatch({ type: 'SET_CONFIG', config });
        dispatch({ type: 'SET_DOCKER', available: docker.available });
      } catch (err) {
        console.error('Failed to initialize:', err);
      } finally {
        dispatch({ type: 'SET_LOADING', loading: false });
      }
    }
    init();
  }, []);

  return (
    <AppContext.Provider value={{ state, dispatch, refreshAgents }}>
      {children}
    </AppContext.Provider>
  );
}

export function useApp() {
  return useContext(AppContext);
}
