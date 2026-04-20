import React from 'react';
import { HashRouter, Routes, Route, NavLink } from 'react-router-dom';
import { AppProvider, useApp } from './context/AppContext';
import { OnboardingModal } from './components/OnboardingModal';
import { SettingsPanel } from './components/SettingsPanel';
import { Dashboard } from './pages/Dashboard';
import { AgentDetail } from './pages/AgentDetail';

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { hasError: boolean; error: string }
> {
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { hasError: false, error: '' };
  }
  static getDerivedStateFromError(error: Error) {
    return { hasError: true, error: error.message };
  }
  render() {
    if (this.state.hasError) {
      return (
        <div className="flex flex-col items-center justify-center min-h-screen gap-4">
          <div className="text-[var(--status-error)] text-lg">Something went wrong</div>
          <div className="text-[var(--text-muted)] text-sm">{this.state.error}</div>
          <button onClick={() => window.location.reload()}
            className="px-4 py-2 bg-[var(--accent)] text-black rounded-lg hover:bg-[var(--accent-hover)]">Reload</button>
        </div>
      );
    }
    return this.props.children;
  }
}

function TopNav() {
  const linkBase = 'px-4 py-2 text-sm font-medium rounded-lg transition-colors';
  const linkInactive = 'text-[var(--text-muted)] hover:text-[var(--text-primary)]';
  const linkActive = 'text-[var(--accent)]';
  return (
    <nav
      className="flex items-center gap-2 px-6 py-3 border-b"
      style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
    >
      <NavLink to="/" end className={({ isActive }) => `${linkBase} ${isActive ? linkActive : linkInactive}`}>
        Habitat
      </NavLink>
      <NavLink to="/settings" className={({ isActive }) => `${linkBase} ${isActive ? linkActive : linkInactive}`}>
        Settings
      </NavLink>
    </nav>
  );
}

function AppContent() {
  const { state } = useApp();

  if (state.loading) {
    return (
      <div className="flex items-center justify-center min-h-screen">
        <div className="text-[var(--text-muted)]">Loading...</div>
      </div>
    );
  }

  if (state.config && !state.config.ui.onboarding_complete) {
    return <OnboardingModal />;
  }

  return (
    <HashRouter>
      <div className="flex flex-col min-h-screen">
        <TopNav />
        <main className="flex-1 p-6">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/settings" element={<SettingsPanel />} />
            <Route path="/agent/:id" element={<AgentDetail />} />
          </Routes>
        </main>
      </div>
    </HashRouter>
  );
}

export default function App() {
  return (
    <ErrorBoundary>
      <AppProvider>
        <AppContent />
      </AppProvider>
    </ErrorBoundary>
  );
}
